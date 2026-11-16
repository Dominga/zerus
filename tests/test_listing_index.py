"""Ordinary ls acceleration, private state and an isolated empty tmux socket."""
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import time
import unittest
import uuid

if os.name == 'posix':
    import fcntl

HGS = Path(os.environ.get('HGS_TEST_BIN', Path(__file__).resolve().parents[1] / 'target/debug/hgs')).resolve()


@unittest.skipUnless(sys.platform.startswith('linux') and shutil.which('tmux'), 'qualified Linux listing activation')
class ListingIndex(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix='zli-')
        self.root = Path(self.temp.name).resolve()
        for name in ['state', 'config', 'tmp', 'tmux']:
            (self.root / name).mkdir(mode=0o700)
        self.state = self.root / 'state'
        self.env = {k:v for k,v in os.environ.items() if not k.startswith('HGS_') and k not in ('TMUX','TMUX_PANE','CLAUDE_CONFIG_DIR','CODEX_HOME','KIMI_CODE_HOME','DEEPSEEK_HOME','OPENCODE_CONFIG','OPENCODE_CONFIG_DIR')}
        self.env.update(HOME=str(self.root), HGS_STATE_DIR=str(self.state),
                        HGS_CONFIG_DIR=str(self.root/'config'), TMPDIR=str(self.root/'tmp'),
                        TMUX_TMPDIR=str(self.root/'tmux'), HGS_SELF='example', HGS_PEERS='',
                        XDG_CONFIG_HOME=str(self.root/'xdg-config'), XDG_STATE_HOME=str(self.root/'xdg-state'), XDG_DATA_HOME=str(self.root/'xdg-data'))

    def tearDown(self):
        directory = self.state/'listing-metadata'
        if directory.is_dir():
            (directory/'disabled').write_text('1\n')
            deadline = time.monotonic()+5
            while time.monotonic() < deadline:
                try:
                    with (directory/'worker.lock').open('rb') as handle:
                        fcntl.flock(handle, fcntl.LOCK_EX | fcntl.LOCK_NB)
                    break
                except FileNotFoundError:
                    break
                except BlockingIOError:
                    time.sleep(.02)
            else:
                self.fail('private metadata worker did not release its resources after disable')
        self.temp.cleanup()

    def call(self, *args):
        return subprocess.run([str(HGS),*args],env=self.env,capture_output=True,text=True,timeout=12)

    def saved(self, n):
        name=f'codex/example/session-{n:04}'
        record=dict(version=1, name=name, agent='codex', run_id=f'run-{n}',
                    session_id=str(uuid.uuid4()), conversation_id=str(uuid.uuid4()),
                    created=n, updated=n, phase='interrupted', activity='idle',
                    cwd='/srv/example', base=['codex'], prompt='Public request')
        path=self.state/(hashlib.sha256(name.encode()).hexdigest()+'.json')
        path.write_text(json.dumps(record))
        return record,path

    def ready(self):
        enabled=self.call('listing-index','enable','--json')
        self.assertEqual(enabled.returncode,0,enabled.stderr)
        self.assertEqual(json.loads(enabled.stdout)['state'],'building')
        deadline=time.monotonic()+8
        while time.monotonic()<deadline:
            status=self.call('listing-index','status','--json')
            if status.returncode==0 and json.loads(status.stdout)['state']=='ready':return
            time.sleep(.05)
        self.fail('independent bootstrap never became ready')

    def complete(self, *extra):
        deadline=time.monotonic()+8;reply=None
        while time.monotonic()<deadline:
            reply=self.call('ls','--json','--local','--no-archives',*extra)
            if reply.returncode==0:return json.loads(reply.stdout)
            time.sleep(.05)
        self.fail(reply.stderr if reply else 'no listing')

    def canonical(self):
        reply=subprocess.run([str(HGS),'__state','merge'],input=json.dumps(dict(host='example',sessions=[])),
                             env=self.env,capture_output=True,text=True,timeout=12)
        self.assertEqual(reply.returncode,0,reply.stderr)
        return json.loads(reply.stdout)['sessions']

    def native(self, agent, *, id_layout=False):
        name=f'{agent}/example/native'
        record=dict(name=name,agent=agent,backend=agent,run_id='native-run',session_id=str(uuid.uuid4()),
                    conversation_id='session-native' if agent=='dsh' else 'ses_native',
                    created=1,cwd='/srv/example',agent_home=str(self.root/'native-home'))
        folder=self.state/agent/('sessions' if id_layout else 'bindings');folder.mkdir(parents=True,exist_ok=True)
        if id_layout:record['layout']='id'
        path=folder/((record['session_id'] if id_layout else hashlib.sha256(name.encode()).hexdigest())+'.json')
        path.write_text(json.dumps(record));return record,path

    def assert_rows(self, expected, actual):
        def stable(rows):
            def remove_clock(value):
                if isinstance(value,dict):return {k:remove_clock(v) for k,v in value.items() if k not in ('sampled_at','goal_observed_at')}
                if isinstance(value,list):return [remove_clock(v) for v in value]
                return value
            return {row['name']:remove_clock(row) for row in rows}
        def clocks(rows):
            found={}
            def visit(value,path):
                if isinstance(value,dict):
                    for key,item in value.items():
                        if key in ('sampled_at','goal_observed_at'):
                            self.assertIsInstance(item,(int,float));found[path+(key,)]=type(item).__name__
                        else:visit(item,path+(key,))
                elif isinstance(value,list):
                    for index,item in enumerate(value):visit(item,path+(index,))
            for row in rows:visit(row,(row['name'],))
            return found
        self.assertEqual(clocks(expected),clocks(actual),'observation clock presence/type changed')
        a,b=stable(expected),stable(actual)
        self.assertEqual(set(a),set(b))
        differences={name:[key for key in set(a[name])|set(b[name]) if a[name].get(key)!=b[name].get(key)] for name in a if a[name]!=b[name]}
        self.assertFalse(differences,f'public rows differ: {differences}')

    def test_large_default_off_preflight_precedes_recovery_body_reads(self):
        for n in range(300):self.saved(n)
        folder=self.state/'renames';folder.mkdir()
        source=folder/'pending.json';source.write_text('{"version":999}')
        result=self.call('ls','--json','--local','--no-archives')
        self.assertNotEqual(result.returncode,0)
        self.assertEqual(result.stdout,'')
        self.assertIn('bounded default-off inventory',result.stderr)
        self.assertEqual(source.read_text(),'{"version":999}')

    def test_unavailable_native_and_terminal_rows_match_the_canonical_projection(self):
        self.saved(1)
        self.native('dsh',id_layout=True)
        self.native('opencode')
        expected=self.canonical()
        self.ready()
        self.assert_rows(expected,self.complete()['sessions'])

    def test_complete_saved_public_rows_cover_the_summary_and_private_transition_matrix(self):
        for n,case in enumerate(['recipe','error','settings','quiet','system-prompt','id','children','account','fork','provider-error','transition','unknown','kimi-main','claude-internal','announced-child','external-child','kimi-roster']):
            record,path=self.saved(n)
            record.update(model='example-model',effort='medium',last_message='Public reply',reply_id=f'reply-{n}',last_reply_at=1)
            if case=='error':record['error']='Native fixture failed'
            if case=='settings':record['pending_settings']=dict(request_id='settings-1',model='next',effort='high');record['expected_id']='waiting'
            if case=='quiet':record['keep_warm_quiet']=dict(prompt='Original request',last_message='Original reply')
            if case=='system-prompt':record['prompt']='<task-notification><task-id>example</task-id><summary>Done</summary><result>'+('x'*600)+'</result></task-notification>'
            if case=='id':
                path.unlink();folder=self.state/'sessions';folder.mkdir(exist_ok=True);path=folder/(record['session_id']+'.json');record['layout']='id'
            if case=='children':record['subagents']=dict(child=dict(name='Worker',state='working',prompt='Child work',started=12))
            if case=='account':record['account_binding']=dict(id='fixture-account',agent='codex',home=str(self.root/'account'))
            if case=='fork':record.update(fork_parent_id='parent-conversation',fork_source_name='codex/example/source')
            if case=='provider-error':record['provider_error']=dict(kind='authentication',detail='Native authentication failed',at=9)
            if case=='transition':record['pausing']=dict(private='never-cache-confirmation')
            if case=='unknown':record.update(phase='unknown',activity='unknown')
            if case in ('kimi-main','claude-internal'):
                agent='kimi' if case=='kimi-main' else 'claude';path.unlink();record.update(agent=agent,name=record['name'].replace('codex/',agent+'/'),base=[agent],phase='working',activity='busy',main_done=True,active_tools={})
                record['subagents']={'main':dict(name='main',state='working')} if agent=='kimi' else {'internal':dict(name='internal',state='working')}
                path=self.state/(hashlib.sha256(record['name'].encode()).hexdigest()+'.json')
            if case in ('announced-child','external-child'):
                record.update(phase='working',activity='busy',main_done=True)
                record['subagents']={'child':dict(name='child',state='working',announced=case=='announced-child',external=case=='external-child',updated=1,started=1)}
            if case=='kimi-roster':
                path.unlink();record.update(agent='kimi',name=record['name'].replace('codex/','kimi/'),base=['kimi'],kimi_native_roster=True,subagent_groups_complete=True)
                path=self.state/(hashlib.sha256(record['name'].encode()).hexdigest()+'.json')
            path.write_text(json.dumps(record))
        expected=self.canonical();self.ready();self.assert_rows(expected,self.complete()['sessions'])

    def test_parent_outside_the_requested_page_keeps_its_state(self):
        parent,_=self.saved(1);child,path=self.saved(2)
        child['parent']=dict(schema=1,machine='11111111-1111-4111-8111-111111111111',host='example',
                             name=parent['name'],session_id=parent['session_id'],run_id=parent['run_id'],
                             conversation_id=parent['conversation_id'],source='manual',at=1)
        path.write_text(json.dumps(child));expected=self.canonical();self.ready()
        page=self.complete('--paged','--page-size','1');rows=page['sessions']
        token=page['listing_page']['next_cursor']
        while token:
            page=self.complete('--paged','--page-size','1','--cursor',token)
            rows.extend(page['sessions']);token=page['listing_page']['next_cursor']
        self.assert_rows(expected,rows)

    def test_resident_deepseek_rows_keep_reply_and_person_attention(self):
        import socket
        import threading
        record,path=self.native('dsh')
        record['parent']=dict(schema=1,machine='11111111-1111-4111-8111-111111111111',host='example',
                              name='claude/example/lead',session_id=str(uuid.uuid4()),run_id='lead-run',conversation_id='lead-conversation',source='launch',at=1)
        path.write_text(json.dumps(record))
        host=self.state/'dsh/hosts'/hashlib.sha256(record['agent_home'].encode()).hexdigest()[:16]
        host.mkdir(parents=True)
        server=socket.socket(socket.AF_UNIX);server.bind(str(host/'host.sock'));server.listen();server.settimeout(.05)
        stop=threading.Event();requests=[]
        item=dict(sessionId=record['conversation_id'],agentAvailable=True,running=False,updatedAt=1000000,
                  projections=dict(values=dict(turnOutline=[dict(turn=3,seq=3,prompt='Public request',response='Public reply')])),
                  pendingQuestions=[dict(id='person-question',questions=[],approval=True)])
        def serve():
            while not stop.is_set():
                try:client,_=server.accept()
                except socket.timeout:continue
                with client:
                    request=json.loads(client.makefile('rb').readline());requests.append(request['method'])
                    value=dict(generation='private-generation',items=[item]) if request['method']=='list' else {}
                    client.sendall((json.dumps(dict(ok=True,value=value))+'\n').encode())
        thread=threading.Thread(target=serve,daemon=True);thread.start()
        try:
            expected=self.canonical();self.ready();actual=self.complete()['sessions']
            self.assert_rows(expected,actual)
            row=next(row for row in actual if row['name']==record['name'])
            self.assertEqual((row['phase'],row['human_request']),('input',True))
            self.assertEqual(row['reply_id'],'dsh-turn-3')
            self.assertTrue(set(requests)<=set(['list','inspect']))
            self.assertEqual(json.loads(path.read_text()),record)
        finally:
            stop.set();thread.join(timeout=2);server.close()

    def test_opencode_current_and_legacy_rows_use_the_same_readonly_normalizer(self):
        import http.server
        import threading
        document=json.loads((Path(__file__).resolve().parent/'fixtures/opencode2/openapi-2.0.32.json').read_text())
        current,path=self.native('opencode');current['service']='opencode2';path.write_text(json.dumps(current))
        legacy=dict(current,name='opencode/example/legacy',session_id=str(uuid.uuid4()),conversation_id='ses_legacy')
        legacy.pop('service');legacy_path=path.parent/(hashlib.sha256(legacy['name'].encode()).hexdigest()+'.json');legacy_path.write_text(json.dumps(legacy))
        seen=[]
        class Server(http.server.BaseHTTPRequestHandler):
            def log_message(self,*_):pass
            def do_GET(self):
                seen.append(self.path);code=200
                if self.path=='/api/info':value=dict(version='2.0.32',pid=os.getpid(),urls=[])
                elif self.path=='/openapi.json':value=document
                elif self.path=='/api/session/active':value=dict(data={})
                elif self.path=='/api/experimental/migration/v1':value=dict(data=dict(status='completed'))
                elif self.path=='/api/session/ses_native':value=dict(data=dict(id='ses_native',location=dict(directory=current['cwd'])))
                elif self.path=='/api/session/ses_legacy':code=404;value=dict(message='Not found')
                elif self.path.startswith('/api/session/ses_native/'):
                    value=dict(data=[],nextCursor=None)
                else:code=404;value=dict(message='Not found')
                raw=json.dumps(value).encode();self.send_response(code);self.send_header('Content-Type','application/json');self.send_header('Content-Length',str(len(raw)));self.end_headers();self.wfile.write(raw)
            def do_POST(self):
                seen.append('POST '+self.path);self.send_error(405)
        server=http.server.ThreadingHTTPServer(('127.0.0.1',0),Server);thread=threading.Thread(target=server.serve_forever,daemon=True);thread.start()
        registration=Path(self.env['XDG_STATE_HOME'])/'opencode/service.json';registration.parent.mkdir(parents=True)
        registration.write_text(json.dumps(dict(id='private-service',pid=os.getpid(),url=f'http://127.0.0.1:{server.server_port}',version='2.0.32',password=None)))
        try:
            expected=self.canonical();self.ready();actual=self.complete()['sessions'];self.assert_rows(expected,actual)
            row=next(row for row in actual if row['name']==legacy['name'])
            self.assertEqual(row['activity_summary'],'OpenCode 1 conversation')
            self.assertEqual(json.loads(path.read_text()),current);self.assertEqual(json.loads(legacy_path.read_text()),legacy)
            self.assertFalse(any(request.startswith('POST ') for request in seen))
        finally:server.shutdown();server.server_close();thread.join(timeout=2)

    def test_small_default_off_listing_keeps_full_public_rows_without_an_index(self):
        self.saved(1);self.saved(2);expected=self.canonical()
        self.assert_rows(expected,self.complete()['sessions'])
        self.assertFalse((self.state/'listing-metadata').exists())

    def test_default_off_large_inventory_refuses_before_full_record_loading(self):
        for n in range(300):
            self.saved(n)
        reply=self.call('ls','--json','--local','--no-archives')
        self.assertNotEqual(reply.returncode,0,'large default-off ls silently loaded the whole inventory')
        self.assertEqual(reply.stdout,'','a refused complete listing leaked a partial body')
        self.assertIn('listing',reply.stderr.lower())
        self.assertFalse((self.state/'listing-metadata').exists(),'default-off created an index')

    def test_enabled_corrupt_index_never_falls_back_to_whole_records(self):
        record,path=self.saved(1)
        original=path.read_bytes()
        directory=self.state/'listing-metadata';directory.mkdir(mode=0o700)
        (directory/'enabled').write_text('1\n')
        (directory/'enabled').chmod(0o600)
        (directory/'index.sqlite3').write_bytes(b'corrupt private display index')
        with (directory/'worker.lock').open('wb') as lease:
            fcntl.flock(lease,fcntl.LOCK_EX|fcntl.LOCK_NB)
            reply=self.call('ls','--json','--local','--no-archives')
        self.assertNotEqual(reply.returncode,0,'enabled corrupt index used a whole-record rescue')
        self.assertEqual(reply.stdout,'')
        self.assertEqual(path.read_bytes(),original)

    def test_byte_preflight_and_late_response_limits_never_publish_partial_json(self):
        for n in range(2):
            record,path=self.saved(n);record['private_blob']='x'*(9<<20);path.write_text(json.dumps(record))
        reply=self.call('ls','--json','--local','--no-archives')
        self.assertNotEqual(reply.returncode,0);self.assertEqual(reply.stdout,'')
        self.assertIn('byte budget',reply.stderr)
        for path in self.state.glob('*.json'):path.unlink()
        self.saved(3);self.ready()
        (self.root/'config/projects.local').write_text(''.join(f'project-{n}=/srv/example/'+('x'*1024)+'\n' for n in range(1024)))
        reply=self.call('ls','--json','--local','--no-archives','--paged')
        self.assertNotEqual(reply.returncode,0);self.assertEqual(reply.stdout,'')
        self.assertIn('response bound',reply.stderr)

    def test_invalid_pages_refuse_without_changing_the_canonical_record(self):
        _,path=self.saved(1);before=path.read_bytes();self.ready()
        for extra in [('--paged','--page-size','0'),('--cursor','invalid'),('--paged','--cursor','invalid')]:
            result=self.call('ls','--json','--local','--no-archives',*extra)
            self.assertNotEqual(result.returncode,0);self.assertEqual(result.stdout,'')
        self.assertEqual(path.read_bytes(),before)

    def test_a_poisoned_display_completion_cannot_authorize_canonical_archival(self):
        import sqlite3
        import contextlib
        _,path=self.saved(1);original=path.read_bytes();self.ready()
        database=self.state/'listing-metadata/index.sqlite3'
        with contextlib.closing(sqlite3.connect(database,timeout=1)) as connection:
            source,raw=connection.execute('SELECT source,seed FROM sources WHERE tombstone=0').fetchone()
            envelope=json.loads(raw);hint=envelope['record']
            hint.update(phase='ended',conversation_state='ended',completion_source='supervisor',completion_reason='clean_exit',exit_code=0,termination_signal=None)
            hint['session_end']=dict(run_id=hint['run_id'],conversation_id=hint['conversation_id'],reason='other',at=1)
            encoded=json.dumps(hint,sort_keys=True,separators=(',',':'),ensure_ascii=False)
            envelope['digest']=hashlib.sha256(encoded.encode()).hexdigest()
            connection.execute('UPDATE sources SET seed=? WHERE source=?',(json.dumps(envelope,separators=(',',':'),sort_keys=True),source))
            connection.commit()
        deadline=time.monotonic()+3
        while time.monotonic()<deadline:
            self.assertTrue(path.exists(),'advisory completion evidence retired the canonical binding')
            self.assertEqual(path.read_bytes(),original)
            self.assertFalse(list((self.state/'archives').glob('*.json')))
            time.sleep(.05)

    def test_background_canonical_reconciliation_keeps_pending_termination(self):
        paths=[]
        for n in range(2):
            record,path=self.saved(n);old=record['name'];record.update(agent='claude',name=old.replace('codex/','claude/'),phase='ended',conversation_state='ended',base=['claude'],pid=None,process_start=None,exit_code=0)
            record['session_end']=dict(run_id=record['run_id'],conversation_id=record['conversation_id'],reason='prompt_input_exit',at=1)
            if n==1:record['terminating']=dict(request_id='original-termination',unknown=True)
            path.unlink();path=self.state/(hashlib.sha256(record['name'].encode()).hexdigest()+'.json');path.write_text(json.dumps(record));paths.append(path)
        self.call('listing-index','enable','--json')
        deadline=time.monotonic()+8
        while paths[0].exists() and time.monotonic()<deadline:time.sleep(.05)
        self.assertFalse(paths[0].exists(),'canonical clean exit never reconciled outside ls/recovery policy')
        self.assertTrue(paths[1].exists(),'display worker retired a pending termination')
        self.assertEqual(len(list((self.state/'archives').glob('*.json'))),1)

    def test_explicit_pages_have_a_coverage_contract(self):
        for n in range(12):
            self.saved(n)
        enabled=self.call('listing-index','enable','--json')
        self.assertEqual(enabled.returncode,0,enabled.stderr)
        self.assertEqual(json.loads(enabled.stdout)['state'],'building')
        deadline=time.monotonic()+8
        while time.monotonic()<deadline:
            status=self.call('listing-index','status','--json')
            if status.returncode==0 and json.loads(status.stdout)['state']=='ready':
                break
            time.sleep(.05)
        else:
            self.fail('independent bootstrap never became ready')
        deadline=time.monotonic()+8
        while time.monotonic()<deadline:
            reply=self.call('ls','--json','--local','--no-archives','--paged')
            if reply.returncode==0:
                break
            self.assertIn('listing',reply.stderr.lower())
            time.sleep(.05)
        self.assertEqual(reply.returncode,0,reply.stderr)
        value=json.loads(reply.stdout)
        self.assertIn('listing_page',value,'--paged was ignored and returned a legacy body')
        self.assertEqual(value['listing_page']['abi'],'session-listing-metadata-v1')
        self.assertTrue(value['listing_page']['complete'])
        self.assertIsNone(value['listing_page']['next_cursor'])
        self.assertEqual(len(value['sessions']),12)

    def test_worker_coalesces_releases_native_fds_and_resumes_after_crash(self):
        self.saved(1)
        directory=self.state/'listing-metadata';directory.mkdir(mode=0o700)
        marker=directory/'enabled';marker.write_text('1\n');marker.chmod(0o600)
        workers=[subprocess.Popen([str(HGS),'__state','listing-metadata-worker'],env=self.env,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL) for _ in range(8)]
        try:
            deadline=time.monotonic()+8
            while time.monotonic()<deadline:
                active=[worker for worker in workers if worker.poll() is None]
                status=directory/'worker-status.json'
                if len(active)==1 and status.exists() and json.loads(status.read_text()).get('state')=='ready':break
                time.sleep(.05)
            else:self.fail('worker lease did not coalesce eight contenders to one ready worker')
            worker=active[0]
            descriptors=list(Path(f'/proc/{worker.pid}/fd').iterdir())
            self.assertLessEqual(len(descriptors),32)
            feeds=[p for p in descriptors if os.readlink(p)=='anon_inode:inotify']
            self.assertEqual(len(feeds),1)
            info=Path(f'/proc/{worker.pid}/fdinfo/{feeds[0].name}').read_text()
            self.assertLessEqual(sum(line.startswith('inotify wd:') for line in info.splitlines()),16)
            # Only this private worker, owned as a subprocess, is interrupted.
            worker.terminate();worker.wait(timeout=4)
            replacement=subprocess.Popen([str(HGS),'__state','listing-metadata-worker'],env=self.env,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
            workers.append(replacement)
            self.assertEqual(len(self.complete()['sessions']),1)
            (directory/'disabled').write_text('1\n')
            replacement.wait(timeout=4)
            self.assertFalse(Path(f'/proc/{replacement.pid}/fd').exists())
        finally:
            (directory/'disabled').write_text('1\n')
            for worker in workers:
                worker.wait(timeout=5)

    def test_worker_exits_on_its_own_executable_replacement(self):
        self.saved(1)
        directory=self.state/'listing-metadata';directory.mkdir(mode=0o700)
        marker=directory/'enabled';marker.write_text('1\n');marker.chmod(0o600)
        copied=self.root/'private-hgs';shutil.copy2(HGS,copied)
        worker=subprocess.Popen([str(copied),'__state','listing-metadata-worker'],env=self.env,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
        try:
            deadline=time.monotonic()+8
            while not (directory/'worker-status.json').exists() and time.monotonic()<deadline:time.sleep(.05)
            self.assertIsNone(worker.poll())
            replacement=self.root/'replacement-hgs';shutil.copy2(HGS,replacement);os.replace(replacement,copied)
            worker.wait(timeout=4)
        finally:
            (directory/'disabled').write_text('1\n');worker.wait(timeout=5)


import test_pause

@unittest.skipUnless(sys.platform.startswith('linux') and shutil.which('tmux'), 'qualified Linux terminal listing')
class LiveListingIndex(test_pause.Harness):
    def setUp(self):
        super().setUp()
        self.env.update(XDG_CONFIG_HOME=str(self.root/'xdg-config'), XDG_STATE_HOME=str(self.root/'xdg-state'), XDG_DATA_HOME=str(self.root/'xdg-data'))

    def tearDown(self):
        directory=self.root/'state/listing-metadata'
        if directory.is_dir():
            (directory/'disabled').write_text('1\n')
            deadline=time.monotonic()+5
            while time.monotonic()<deadline:
                try:
                    with (directory/'worker.lock').open('rb') as handle:fcntl.flock(handle,fcntl.LOCK_EX|fcntl.LOCK_NB)
                    break
                except FileNotFoundError:break
                except BlockingIOError:time.sleep(.02)
            else:self.fail('private listing worker still holds the fixture')
        super().tearDown()

    def assert_rows(self,expected,actual):ListingIndex.assert_rows(self,expected,actual)

    def projected(self):
        return self.ready_rows()

    def ready_rows(self):
        deadline=time.monotonic()+8
        while time.monotonic()<deadline:
            result=subprocess.run([str(HGS),'ls','--json','--local','--no-archives'],env=self.env,capture_output=True,text=True,timeout=12)
            if result.returncode==0:return json.loads(result.stdout)['sessions']
            time.sleep(.05)
        self.fail(result.stderr)

    def test_live_terminal_id_rename_reply_and_attention_keep_all_public_fields(self):
        name=self.start('claude','listing')
        self.send_event(name,'UserPromptSubmit',prompt='A public request')
        self.send_event(name,'Stop',last_assistant_message='Public reply')
        twin=self.start('codex','id-listing');self.move_to_id_layout(twin)
        expected=self.projected()
        self.hgs('listing-index','enable','--json')
        actual=self.ready_rows();self.assert_rows(expected,actual)
        self.assertTrue(next(row for row in actual if row['name']==name).get('reply_id'))
        # Genuine hooks use the existing native producer; no admission bypass.
        self.send_event(name,'PreToolUse',tool_name='Bash',tool_use_id='listing-tool',tool_input={'command':'printf example'})
        self.send_event(name,'PermissionRequest',tool_name='Bash')
        # Disable only this private computer to obtain the canonical compatibility oracle.
        self.hgs('listing-index','disable','--json');expected=self.projected()
        self.hgs('listing-index','enable','--json');self.assert_rows(expected,self.ready_rows())
        self.send_event(name,'PostToolUse',tool_name='Bash',tool_use_id='listing-tool',tool_response={'output':'ok'})
        renamed=name.rsplit('/',1)[0]+'/renamed';self.hgs('rename',name,renamed)
        self.hgs('listing-index','disable','--json');expected=self.projected()
        self.hgs('listing-index','enable','--json');actual=self.ready_rows();self.assert_rows(expected,actual)
        self.assertIn(renamed,[row['name'] for row in actual])


import test_process_control

@unittest.skipUnless(sys.platform.startswith('linux') and shutil.which('tmux'), 'private native process tree')
class BackgroundListingIndex(unittest.TestCase):
    script=test_process_control.ProcessControl.script
    write_record=test_process_control.ProcessControl.write_record
    tmux=test_process_control.ProcessControl.tmux
    wait_for=test_process_control.ProcessControl.wait_for
    setUp=test_process_control.ProcessControl.setUp

    def tearDown(self):
        directory=self.root/'state/listing-metadata'
        if directory.is_dir():
            (directory/'disabled').write_text('1\n')
            deadline=time.monotonic()+5
            while time.monotonic()<deadline:
                try:
                    with (directory/'worker.lock').open('rb') as handle:fcntl.flock(handle,fcntl.LOCK_EX|fcntl.LOCK_NB)
                    break
                except FileNotFoundError:break
                except BlockingIOError:time.sleep(.02)
            else:self.fail('private worker did not settle')
        test_process_control.ProcessControl.tearDown(self)

    def call(self,*args):
        return subprocess.run([str(HGS),*args],env=self.env,text=True,capture_output=True,timeout=12)

    def ready_rows(self):
        deadline=time.monotonic()+8
        while time.monotonic()<deadline:
            reply=self.call('ls','--json','--local','--no-archives')
            if reply.returncode==0:return json.loads(reply.stdout)['sessions']
            time.sleep(.05)
        self.fail(reply.stderr)

    def test_background_unknown_and_recorded_jobs_match_the_canonical_live_rows(self):
        import signal
        self.record.update(phase='idle',activity='idle',active_tools={},reply_id='public-reply',last_reply_at=1,last_message='Public reply')
        self.write_record()
        original=self.record['process_start']
        for state in ['background','unknown','historical']:
            with self.subTest(state=state):
                if state=='unknown':self.record['process_start']='another native birth'
                if state=='historical':
                    self.record['process_start']=original
                    for pid in self.children:
                        try:os.killpg(pid,signal.SIGKILL)
                        except ProcessLookupError:pass
                    self.record['shell_jobs']=dict(old=dict(id='old',status='running',background=True,observed=True,source='hooks',run_id=self.run_id,conversation_id=self.conversation_id,command='recorded command'))
                    time.sleep(.1)
                self.write_record()
                disabled=self.call('listing-index','disable','--json');self.assertEqual(disabled.returncode,0,disabled.stderr)
                expected=self.ready_rows()
                enabled=self.call('listing-index','enable','--json');self.assertEqual(enabled.returncode,0,enabled.stderr)
                actual=self.ready_rows();ListingIndex.assert_rows(self,expected,actual)
                row=next(row for row in actual if row['name']==self.name)
                self.assertEqual(row['process_work']['count'],dict(background=2,unknown=None,historical=0)[state])


import test_remote_login_shells

@unittest.skipUnless(sys.platform.startswith('linux'), 'Linux index activation and private fake peer')
class RemoteListingIndex(unittest.TestCase):
    setUp=test_remote_login_shells.RemoteLoginShells.setUp
    hgs=test_remote_login_shells.RemoteLoginShells.hgs
    received=test_remote_login_shells.RemoteLoginShells.received

    def peer_program(self,capable):
        script="""#!/usr/bin/env python3
import json,os,sys
from pathlib import Path
args=sys.argv[1:]
if args==['--help']:
 print('guarded-create-v1\\nCAPABILITY');sys.exit(0)
if args==['--version']:
 print('hgs 9.9.9');sys.exit(0)
root=Path.home();(root/'argv.json').write_text(json.dumps(args));(root/'stdin').write_text(sys.stdin.read())
if '--paged' in args:
 print(json.dumps(dict(host='peer',ok=True,sessions=[dict(name='codex/example/one')],listing_page=dict(abi='session-listing-metadata-v1',coverage='ready',complete=False,generation=1,next_cursor='next-private-token'))))
elif 'listing-index' in args:print(json.dumps(dict(enabled=False,state='disabled')))
else:print(json.dumps(dict(host='peer',ok=True,sessions=[])))
""".replace('CAPABILITY','session-listing-metadata-v1' if capable else '')
        (self.peer/'.local/bin/hgs').write_text(script)
        # Emulate SSH, whose remote process receives none of our HGS_*.
        ssh=test_remote_login_shells.FAKE_SSH.replace("env = dict(os.environ, HOME=", "env = dict({k:v for k,v in os.environ.items() if not k.startswith('HGS_')}, HOME=")
        (self.root/'fake-ssh').write_text(ssh)

    def test_remote_pages_preserve_the_wire_body_cursor_and_noninteractive_route(self):
        self.peer_program(True)
        result=self.hgs('/bin/sh','@peer','ls','--json','--local','--no-archives','--paged','--page-size','1','--cursor','original-private-token')
        self.assertEqual(result.returncode,0,result.stderr)
        value=json.loads(result.stdout)
        self.assertEqual(value['listing_page']['next_cursor'],'next-private-token')
        args,_=self.received();self.assertIn('--paged',args)
        self.assertEqual(args[args.index('--cursor')+1],'original-private-token')
        self.assertNotIn('-t',json.loads((self.root/'ssh-args.json').read_text()))
        result=self.hgs('/bin/sh','@peer','listing-index','status','--json')
        self.assertEqual(result.returncode,0,result.stderr)
        args,_=self.received();self.assertEqual(args,['listing-index','status','--json'])
        self.assertNotIn('-t',json.loads((self.root/'ssh-args.json').read_text()))

    def test_an_old_peer_never_interprets_listing_controls_as_a_launch(self):
        self.peer_program(False)
        for args in [('listing-index','status','--json'),('--expect-machine','11111111-1111-4111-8111-111111111111','listing-index','status','--json')]:
            result=self.hgs('/bin/sh','@peer',*args)
            self.assertNotEqual(result.returncode,0,f'older peer executed an unsupported display command: {args}')
            self.assertFalse((self.peer/'argv.json').exists(),'unsupported command reached the old peer dispatch')


if __name__ == '__main__':
    unittest.main()
