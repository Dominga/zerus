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


def worker_leased(state):
    with (state/'listing-metadata/worker.lock').open('rb') as handle:
        try:fcntl.flock(handle,fcntl.LOCK_EX|fcntl.LOCK_NB)
        except BlockingIOError:return True
    return False


def seed_aggregate(state,generation):
    import sqlite3
    with sqlite3.connect(f'file:{state}/listing-metadata/index.sqlite3?mode=ro',uri=True) as db:
        return db.execute('SELECT count(*),sum(length(CAST(seed AS BLOB))),max(length(CAST(seed AS BLOB))) FROM snapshots WHERE generation=?',(generation,)).fetchone()


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
        directory=self.state/'listing-metadata';directory.mkdir(exist_ok=True);(directory/'disabled').write_text('1\n');(directory/'disabled').chmod(0o600)
        for n in range(300):self.saved(n)
        folder=self.state/'renames';folder.mkdir()
        source=folder/'pending.json';source.write_text('{"version":999}')
        result=self.call('ls','--json','--local','--no-archives')
        self.assertNotEqual(result.returncode,0)
        self.assertEqual(result.stdout,'')
        self.assertIn('bounded default-off inventory',result.stderr)
        self.assertEqual(source.read_text(),'{"version":999}')

    def test_full_private_file_ceiling_is_256_and_explicit_pages_stay_at_16(self):
        for n in range(256):
            record,path=self.saved(n);record['pausing']={};path.write_text(json.dumps(record))
        self.ready();body=self.complete()
        self.assertEqual(len(body['sessions']),256)
        result=self.call('ls','--json','--local','--no-archives','--paged','--page-size','17')
        self.assertNotEqual(result.returncode,0);self.assertEqual(result.stdout,'')
        self.assertIn('file limit',result.stderr)
        record,path=self.saved(256);record['pausing']={};path.write_text(json.dumps(record))
        deadline=time.monotonic()+8
        while time.monotonic()<deadline:
            result=self.call('ls','--json','--local','--no-archives')
            if 'file limit' in result.stderr:break
            self.assertEqual(result.stdout,'');time.sleep(.05)
        self.assertNotEqual(result.returncode,0);self.assertEqual(result.stdout,'')
        self.assertIn('file limit',result.stderr)

    def test_complete_current_v3_cache_upgrades_through_the_real_worker_without_source_writes(self):
        import sqlite3
        sources=[self.saved(n) for n in range(300)]
        self.ready();self.hgs_disabled=self.call('listing-index','disable','--json')
        self.assertEqual(self.hgs_disabled.returncode,0,self.hgs_disabled.stderr)
        directory=self.state/'listing-metadata'
        deadline=time.monotonic()+8
        while time.monotonic()<deadline:
            try:
                with (directory/'worker.lock').open('rb') as probe:fcntl.flock(probe,fcntl.LOCK_EX|fcntl.LOCK_NB)
                break
            except BlockingIOError:time.sleep(.02)
        else:self.fail('private old-format fixture worker did not stop')
        before={str(path):path.read_bytes() for _,path in sources}
        # Faithful v3 PROJECTION fixture, not corruption or a production writer.
        # No canonical file/native stamp changes. v3 did not have live_public.
        with sqlite3.connect(directory/'index.sqlite3') as db:
            for table in ['sources','snapshots']:
                for key,encoded in db.execute(f'SELECT source,seed FROM {table}').fetchall():
                    value=json.loads(encoded);value['version']=3;value['record']['_listing'].pop('live_public',None)
                    value['digest']=hashlib.sha256(json.dumps(value['record'],ensure_ascii=False,sort_keys=True,separators=(',',':')).encode()).hexdigest()
                    db.execute(f'UPDATE {table} SET seed=? WHERE source=?',(json.dumps(value,separators=(',',':')),key))
            self.assertEqual(db.execute('SELECT ready FROM meta').fetchone()[0],1)
        with (directory/'writer.lock').open('rb') as gate:
            fcntl.flock(gate,fcntl.LOCK_EX)
            enabled=self.call('listing-index','enable','--json');self.assertEqual(enabled.returncode,0,enabled.stderr)
            result=self.call('ls','--json','--local','--no-archives')
            self.assertNotEqual(result.returncode,0);self.assertEqual(result.stdout,'')
            self.assertIn('Display seed identity changed',result.stderr)
        body=self.complete();self.assertEqual(len(body['sessions']),300)
        with sqlite3.connect(directory/'index.sqlite3') as db:
            self.assertEqual(db.execute("SELECT count(*) FROM sources WHERE json_extract(seed,'$.version')!=4").fetchone()[0],0)
            generation=db.execute('SELECT generation FROM meta').fetchone()[0]
            self.assertEqual(db.execute("SELECT count(*) FROM snapshots WHERE generation=? AND json_extract(seed,'$.version')!=4",(generation,)).fetchone()[0],0)
        self.assertEqual({str(path):path.read_bytes() for _,path in sources},before)
        print(json.dumps(dict(control='complete-current-v3-runtime-upgrade',sessions=300,generation=generation,canonical_unchanged=True)),flush=True)

    def test_explicit_disable_before_first_listing_persists_on_qualified_linux(self):
        self.saved(1)
        self.assertFalse((self.state/'listing-metadata').exists())
        result=self.call('listing-index','disable','--json');self.assertEqual(result.returncode,0,result.stderr)
        self.assertTrue((self.state/'listing-metadata/disabled').is_file(),'explicit disable was forgotten before automatic bootstrap')
        body=self.complete();self.assertEqual(len(body['sessions']),1)
        self.assertFalse((self.state/'listing-metadata/index.sqlite3').exists())
        self.assertFalse(json.loads(self.call('listing-index','status','--json').stdout)['enabled'])

    def test_resident_worker_keeps_large_incremental_seed_copy_bounded_and_live(self):
        sources=[]
        for n in range(213):
            record,path=self.saved(n)
            record['recovery_user_text']='r'*(8<<10)
            path.write_text(json.dumps(record));sources.append((record,path))
        self.ready()
        previous=json.loads(self.call('listing-index','status','--json').stdout)['generation']
        aggregate=seed_aggregate(self.state,previous)
        self.assertEqual(aggregate[0],213);self.assertGreater(aggregate[1],512<<10)
        states=[];bodies=[]
        for round in range(3):
            record,path=sources[round*91]
            record['last_event_at']=123+round
            staged=path.with_suffix('.fixture-staged')
            staged.write_text(json.dumps(record));staged.replace(path)
            # Observe only: status cannot silently replace a worker that died
            # while cloning an older/uninstrumented atomic writer's new object.
            deadline=time.monotonic()+8
            while time.monotonic()<deadline:
                status=json.loads(self.call('listing-index','status','--json').stdout)
                states.append({key:status.get(key) for key in ['state','generation','reason']})
                if status.get('state')=='ready' and status.get('generation',0)>previous:break
                time.sleep(.02)
            else:self.fail(f'resident incremental worker failed to restore readiness: {states[-3:]}')
            self.assertGreater(status['generation'],previous)
            previous=status['generation']
            self.assertTrue(worker_leased(self.state),'resident worker lost its lease before ls')
            expected={str(path):path.read_bytes() for _,path in sources}
            body=self.complete();rows=body['sessions']
            self.assertEqual(len(rows),213)
            self.assertEqual({row['name'] for row in rows},{record['name'] for record,_ in sources})
            self.assert_rows(self.canonical(),rows)
            self.assertEqual({str(path):path.read_bytes() for _,path in sources},expected,'repair changed canonical bytes')
            self.assertTrue(worker_leased(self.state),'resident worker lost its lease after ls')
            bodies.append(len(json.dumps(body).encode()))
        print(json.dumps(dict(control='resident-incremental-large-seeds',sessions=213,rounds=3,current=status['generation'],observations=len(states),seed_aggregate=aggregate,body_bytes=bodies)),flush=True)

    def test_qualified_linux_ordinary_listing_bootstraps_without_a_switch(self):
        expected={self.saved(n)[0]['name'] for n in range(300)}
        body=self.complete()
        self.assertEqual({row['name'] for row in body['sessions']},expected)
        status=json.loads(self.call('listing-index','status','--json').stdout)
        self.assertTrue(status['enabled']);self.assertEqual(status['state'],'ready')

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

    def test_slow_native_providers_keep_membership_with_one_bounded_observation_clock(self):
        import http.server
        import socket
        import threading
        record,path=self.native('dsh');before=path.read_bytes()
        host=self.state/'dsh/hosts'/hashlib.sha256(record['agent_home'].encode()).hexdigest()[:16];host.mkdir(parents=True)
        server=socket.socket(socket.AF_UNIX);server.bind(str(host/'host.sock'));server.listen();server.settimeout(.05)
        stop=threading.Event();clients=[]
        def reply(client):
            with client:
                client.recv(4096);stop.wait(3)
                try:client.sendall(b'{"ok":true,"value":{"items":[]}}\n')
                except BrokenPipeError:pass
        def serve():
            while not stop.is_set():
                try:client,_=server.accept()
                except socket.timeout:continue
                thread=threading.Thread(target=reply,args=(client,),daemon=True);clients.append(thread);thread.start()
        thread=threading.Thread(target=serve,daemon=True);thread.start()
        try:
            self.ready();self.complete()  # settle lazy public overlays before timing one call
            started=time.monotonic();reply=self.call('ls','--json','--local','--no-archives');elapsed=time.monotonic()-started
            self.assertEqual(reply.returncode,0,reply.stderr);rows=json.loads(reply.stdout)['sessions']
            self.assertLess(elapsed,1.8,'native read ignored the shared one-second deadline')
            self.assertEqual([row['name'] for row in rows],[record['name']]);self.assertEqual(rows[0]['activity'],'unknown')
            self.assertEqual(path.read_bytes(),before)
        finally:
            stop.set();thread.join(timeout=2);server.close()
            for client in clients:client.join(timeout=2)
        path.unlink();record,path=self.native('opencode');record['service']='opencode2';path.write_text(json.dumps(record));before=path.read_bytes()
        stop=threading.Event()
        class Slow(http.server.BaseHTTPRequestHandler):
            def log_message(self,*_):pass
            def do_GET(self):
                stop.wait(3)
                try:self.send_response(200);self.end_headers();self.wfile.write(b'{}')
                except BrokenPipeError:pass
        http=http.server.ThreadingHTTPServer(('127.0.0.1',0),Slow);thread=threading.Thread(target=http.serve_forever,daemon=True);thread.start()
        registration=Path(self.env['XDG_STATE_HOME'])/'opencode/service.json';registration.parent.mkdir(parents=True,exist_ok=True)
        registration.write_text(json.dumps(dict(id='slow-private-service',pid=os.getpid(),url=f'http://127.0.0.1:{http.server_port}',version='2.0.32',password=None)))
        try:
            self.complete()
            started=time.monotonic();reply=self.call('ls','--json','--local','--no-archives');elapsed=time.monotonic()-started
            self.assertEqual(reply.returncode,0,reply.stderr);rows=json.loads(reply.stdout)['sessions']
            self.assertLess(elapsed,1.8,'native HTTP read ignored the shared deadline')
            self.assertEqual([row['name'] for row in rows],[record['name']]);self.assertEqual(path.read_bytes(),before)
        finally:stop.set();http.shutdown();http.server_close();thread.join(timeout=2)

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
        directory=self.state/'listing-metadata';directory.mkdir(exist_ok=True);(directory/'disabled').write_text('1\n');(directory/'disabled').chmod(0o600)
        self.saved(1);self.saved(2);expected=self.canonical()
        self.assert_rows(expected,self.complete()['sessions'])
        self.assertFalse((self.state/'listing-metadata/index.sqlite3').exists())

    def test_default_off_large_inventory_refuses_before_full_record_loading(self):
        directory=self.state/'listing-metadata';directory.mkdir(exist_ok=True);(directory/'disabled').write_text('1\n');(directory/'disabled').chmod(0o600)
        for n in range(300):
            self.saved(n)
        reply=self.call('ls','--json','--local','--no-archives')
        self.assertNotEqual(reply.returncode,0,'large default-off ls silently loaded the whole inventory')
        self.assertEqual(reply.stdout,'','a refused complete listing leaked a partial body')
        self.assertIn('listing',reply.stderr.lower())
        self.assertFalse((self.state/'listing-metadata/index.sqlite3').exists(),'disabled listing created an index')

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
        directory=self.state/'listing-metadata';directory.mkdir(exist_ok=True);(directory/'disabled').write_text('1\n');(directory/'disabled').chmod(0o600)
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

    def measured_full(self):
        import threading
        command=[str(HGS),'ls','--json','--local','--no-archives']
        process=subprocess.Popen(command,env=self.env,stdout=subprocess.PIPE,stderr=subprocess.PIPE,text=True)
        directory=os.open(f'/proc/{process.pid}',os.O_RDONLY|os.O_DIRECTORY)
        done=threading.Event();measure=dict(peak_fds=0,peak_rss_kib=0,read_chars=0,read_bytes=0)
        def sample():
            while not done.is_set():
                try:
                    descriptor=os.open('fd',os.O_RDONLY|os.O_DIRECTORY,dir_fd=directory)
                    try:measure['peak_fds']=max(measure['peak_fds'],len(os.listdir(descriptor)))
                    finally:os.close(descriptor)
                    for filename in ['status','io']:
                        with os.fdopen(os.open(filename,os.O_RDONLY,dir_fd=directory)) as stream:text=stream.read()
                        for line in text.splitlines():
                            if line.startswith('VmRSS:'):measure['peak_rss_kib']=max(measure['peak_rss_kib'],int(line.split()[1]))
                            if line.startswith('rchar:'):measure['read_chars']=max(measure['read_chars'],int(line.split()[1]))
                            if line.startswith('read_bytes:'):measure['read_bytes']=max(measure['read_bytes'],int(line.split()[1]))
                except (FileNotFoundError,ProcessLookupError,PermissionError):pass
                done.wait(.002)
        thread=threading.Thread(target=sample);thread.start()
        try:
            stdout,stderr=process.communicate(timeout=12)
        except subprocess.TimeoutExpired:
            process.kill();process.communicate();raise
        finally:done.set();thread.join(2);os.close(directory)
        self.assertLessEqual(measure['peak_fds'],64,'listing retained unbounded canonical descriptors')
        return subprocess.CompletedProcess(command,process.returncode,stdout,stderr),measure

    def projected(self):
        return self.ready_rows()

    def ready_rows(self):
        deadline=time.monotonic()+8
        while time.monotonic()<deadline:
            result=subprocess.run([str(HGS),'ls','--json','--local','--no-archives'],env=self.env,capture_output=True,text=True,timeout=12)
            if result.returncode==0:return json.loads(result.stdout)['sessions']
            time.sleep(.05)
        self.fail(result.stderr)

    def test_no_live_identity_is_repeated_between_explicit_pages(self):
        live=self.start('claude','paged-live')
        self.send_event(live,'UserPromptSubmit',prompt='Private review fixture')
        self.send_event(live,'Stop',last_assistant_message='Private review reply')
        live_key=hashlib.sha256(live.encode()).hexdigest()
        count=0
        for n in range(10_000):
            name=f'codex/example/review-before-{n}'
            key=hashlib.sha256(name.encode()).hexdigest()
            if key>=live_key:continue
            record=dict(version=1,name=name,agent='codex',run_id=f'review-run-{n}',session_id=str(uuid.uuid4()),conversation_id=str(uuid.uuid4()),created=1,updated=1,phase='interrupted',activity='idle',cwd='/srv/example',base=['codex'],prompt='Public fixture')
            (self.root/'state'/(key+'.json')).write_text(json.dumps(record));count+=1
            if count==3:break
        self.assertEqual(count,3)
        self.hgs('listing-index','enable','--json');expected=self.ready_rows()
        self.assertEqual(len(expected),4)
        def page(cursor=None):
            args=[str(HGS),'ls','--json','--local','--no-archives','--paged','--page-size','1']
            if cursor:args+=['--cursor',cursor]
            r=subprocess.run(args,env=self.env,text=True,capture_output=True,timeout=12)
            self.assertEqual(r.returncode,0,r.stderr);return json.loads(r.stdout)
        first=page();names=[r['name'] for r in first['sessions']];cursor=first['listing_page']['next_cursor'];counts=[len(names)]
        for _ in range(8):
            if not cursor:break
            body=page(cursor);names.extend(r['name'] for r in body['sessions']);counts.append(len(body['sessions']));cursor=body['listing_page']['next_cursor']
        self.assertIsNone(cursor)
        print(json.dumps(dict(live=live,names=names,page_row_counts=counts,expected=[r['name'] for r in expected])),flush=True)
        self.assertEqual(len(names),len(set(names)),'one live native identity appeared on multiple explicit pages')
        self.assertEqual(set(names),{r['name'] for r in expected})

    def page_rows(self):
        deadline=time.monotonic()+8
        while time.monotonic()<deadline:
            rows=[];counts=[];cursor=None;generation=None;retry=False
            for _ in range(32):
                args=['ls','--json','--local','--no-archives','--paged','--page-size','1']
                if cursor:args+=['--cursor',cursor]
                result=subprocess.run([str(HGS),*args],env=self.env,capture_output=True,text=True,timeout=12)
                if result.returncode:
                    self.assertEqual(result.stdout,'')
                    if any(reason in result.stderr for reason in ['database is locked','building, stale or unavailable','still rebuilding','changed before publication']):
                        retry=True;break
                    self.fail(result.stderr)
                raw=result.stdout;self.assertLessEqual(len(raw.encode()),512<<10)
                body=json.loads(raw);page=body['listing_page']
                self.assertEqual(page['coverage'],'ready')
                if generation is None:generation=page['generation']
                self.assertEqual(page['generation'],generation)
                rows.extend(body['sessions']);counts.append(len(body['sessions']))
                cursor=page['next_cursor']
                if not cursor:return rows,counts
            if not retry:self.fail('private page fixture did not finish within32 pages')
            # Discard provisional rows and cursor together; never mix cuts.
            time.sleep(.05)
        self.fail('private listing never admitted a stable paged cut')

    def saved_page_row(self,name,**fields):
        record=dict(version=1,name=name,agent=name.split('/')[0],run_id=str(uuid.uuid4()),session_id=str(uuid.uuid4()),conversation_id=str(uuid.uuid4()),created=1,updated=1,phase='interrupted',activity='idle',cwd='/srv/example',base=['codex'],prompt='Public fixture')
        record.update(fields)
        (self.root/'state').mkdir(exist_ok=True)
        (self.root/'state'/(hashlib.sha256(name.encode()).hexdigest()+'.json')).write_text(json.dumps(record))
        return record

    def native_mux(self,*args):
        result=subprocess.run([str(self.bin/'tmux'),*args],env=self.env,capture_output=True,text=True,timeout=4)
        self.assertEqual(result.returncode,0,result.stderr)
        return result.stdout.strip()

    def test_id_layout_owner_and_untracked_terminal_each_appear_once(self):
        live=self.start('claude','id-page-owner')
        self.send_event(live,'Stop',last_assistant_message='Private ID reply')
        sid,read=self.move_to_id_layout(live)
        # Use the supported ID-twin terminal spelling; the public label remains
        # distinct, so a display-name-only filter cannot locate this owner.
        terminal=live+'~'+sid[:8]
        self.native_mux('rename-session','-t',live,terminal)
        record=read();record['terminal_name']=terminal
        (self.root/'state/sessions'/(sid+'.json')).write_text(json.dumps(record))
        for n in range(3):self.saved_page_row(f'codex/example/id-before-{n}')
        untracked='codex/example/untracked'
        self.native_mux('new-session','-d','-s',untracked,'sleep 120')
        self.hgs('listing-index','enable','--json');expected=self.ready_rows()
        rows,counts=self.page_rows()
        self.assertEqual(len(rows),len(expected))
        self.assert_rows(expected,rows)
        self.assertEqual(sum(row.get('session_id')==sid for row in rows),1)
        self.assertEqual(sum(row['name']==untracked for row in rows),1)
        print(json.dumps(dict(case='id-and-untracked',page_row_counts=counts)),flush=True)
        renamed='claude/p/id-page-renamed'
        self.hgs('rename',live,renamed)
        self.ready_rows();rows,_=self.page_rows()
        self.assertEqual(sum(row.get('session_id')==sid for row in rows),1)
        self.assertIn(renamed,[row['name'] for row in rows])

    def test_same_label_different_native_run_or_pane_stays_untracked(self):
        name='codex/example/native-collision'
        self.native_mux('new-session','-d','-s',name,'sleep 120')
        pane=self.native_mux('display-message','-p','-t',name,'#{pane_id}')
        self.native_mux('set-option','-p','-t',pane,'@hgs_run','native-run')
        for case,fields in [('run',dict(pane=pane,run_id='saved-run')),('pane',dict(pane='%wrong',run_id='native-run'))]:
            with self.subTest(case=case):
                self.hgs('listing-index','disable','--json')
                record=self.saved_page_row(name,**fields)
                self.hgs('listing-index','enable','--json');expected=self.ready_rows()
                self.assertEqual(len(expected),2)
                rows,_=self.page_rows()
                self.assert_rows(expected,rows)
                self.assertEqual(len(rows),2,'a same-label native terminal was hidden')
                self.assertEqual(sum(row.get('session_id')==record['session_id'] for row in rows),1)
                self.assertEqual(sum(not row.get('session_id') for row in rows),1)

    def test_native_locator_failure_keeps_the_page_empty_and_agent_alive(self):
        live=self.start('claude','bounded-page-locator')
        self.send_event(live,'Stop',last_assistant_message='Private locator reply')
        live_key=hashlib.sha256(live.encode()).hexdigest()
        for n in range(10000):
            name=f'codex/example/probe-before-{n}'
            if hashlib.sha256(name.encode()).hexdigest()<live_key:
                self.saved_page_row(name);break
        else:self.fail('private locator fixture had no earlier metadata row')
        self.hgs('listing-index','enable','--json');self.ready_rows()
        original=self.bin/'original-private-tmux';shutil.copy2(self.bin/'tmux',original)
        self.script('tmux',"#!/usr/bin/env python3\nimport os,sys,time\nif any('HGS_RESUME_SESSION_ID' in word for word in sys.argv):\n if os.environ['ZERUS_TEST_LOCATOR']=='slow':time.sleep(3)\n else:sys.stdout.write('x'*9000)\n sys.exit(0)\nos.execv("+repr(str(original))+",["+repr(str(original))+"]+sys.argv[1:])\n")
        before=self.binding(live)
        for mode in ['slow','large']:
            with self.subTest(mode=mode):
                env=dict(self.env,ZERUS_TEST_LOCATOR=mode)
                started=time.monotonic()
                result=subprocess.run([str(HGS),'ls','--json','--local','--no-archives','--paged','--page-size','1'],env=env,capture_output=True,text=True,timeout=4)
                self.assertLess(time.monotonic()-started,2.5)
                self.assertNotEqual(result.returncode,0)
                self.assertEqual(result.stdout,'')
                self.assertIn('Native display locator is unavailable',result.stderr)
                self.assertEqual(self.binding(live),before)
                os.kill(before['pid'],0)

    def test_seventeen_deferred_live_owners_refuse_the_original_overlay_limit(self):
        owners=[]
        for n in range(17):
            name=self.start('claude',f'deferred-{n}')
            self.move_to_id_layout(name);owners.append(name)
        self.saved_page_row('codex/example/first-page')
        self.hgs('listing-index','enable','--json')
        # Full listing has the same original selected-overlay cap; readiness is
        # observed without asking it to project all seventeen owners.
        self.wait(lambda:json.loads(self.hgs('listing-index','status','--json'))['state']=='ready')
        deadline=time.monotonic()+8
        while time.monotonic()<deadline:
            result=subprocess.run([str(HGS),'ls','--json','--local','--no-archives','--paged','--page-size','1'],env=self.env,capture_output=True,text=True,timeout=12)
            if not any(reason in result.stderr for reason in ['database is locked','building, stale or unavailable','still rebuilding']):break
            self.assertEqual(result.stdout,'');time.sleep(.05)
        self.assertNotEqual(result.returncode,0)
        self.assertEqual(result.stdout,'')
        self.assertIn('Selected listing overlays exceeded their file limit',result.stderr)
        for name in owners:os.kill(self.binding(name)['pid'],0)

    def test_slow_provider_and_deferred_live_owner_keep_both_memberships(self):
        import socket,threading
        live=self.start('claude','provider-deferred');self.move_to_id_layout(live)
        state=self.root/'state';home=self.root/'private-dsh-home'
        name='dsh/example/slow-provider'
        record=dict(name=name,agent='dsh',backend='dsh',run_id='native-run',session_id=str(uuid.uuid4()),conversation_id='session-native',created=1,cwd='/srv/example',agent_home=str(home))
        folder=state/'dsh/bindings';folder.mkdir(parents=True)
        path=folder/(hashlib.sha256(name.encode()).hexdigest()+'.json');path.write_text(json.dumps(record));before=path.read_bytes()
        host=state/'dsh/hosts'/hashlib.sha256(str(home).encode()).hexdigest()[:16];host.mkdir(parents=True)
        server=socket.socket(socket.AF_UNIX);server.bind(str(host/'host.sock'));server.listen();server.settimeout(.05)
        stop=threading.Event();clients=[]
        def reply(client):
            with client:
                client.recv(4096);stop.wait(3)
                try:client.sendall(b'{"ok":true,"value":{"items":[]}}\n')
                except BrokenPipeError:pass
        def serve():
            while not stop.is_set():
                try:client,_=server.accept()
                except socket.timeout:continue
                thread=threading.Thread(target=reply,args=(client,),daemon=True);clients.append(thread);thread.start()
        thread=threading.Thread(target=serve,daemon=True);thread.start()
        try:
            self.hgs('listing-index','enable','--json');self.ready_rows()
            rows,_=self.page_rows()
            self.assertEqual({row['name'] for row in rows},{live,name})
            self.assertEqual(len(rows),2)
            self.assertEqual(next(row for row in rows if row['name']==name)['activity'],'unknown')
            self.assertEqual(path.read_bytes(),before)
        finally:
            stop.set();thread.join(timeout=2);server.close()
            for client in clients:client.join(timeout=2)

    def test_resident_large_seed_worker_follows_a_genuine_hook_writer_across_three_cuts(self):
        name=self.start('claude','incremental-seed-writer')
        self.state=self.root/'state'
        path=self.state/(hashlib.sha256(name.encode()).hexdigest()+'.json')
        sources=[(self.binding(name),path)]+[ListingIndex.saved(self,n) for n in range(212)]
        for record,path in sources:
            record['recovery_user_text']='r'*(8<<10);path.write_text(json.dumps(record))
        self.hgs('listing-index','enable','--json')
        def status():return json.loads(self.hgs('listing-index','status','--json'))
        self.wait(lambda:status()['state']=='ready')
        previous=status()['generation'];aggregate=seed_aggregate(self.state,previous)
        self.assertEqual(aggregate[0],213);self.assertGreater(aggregate[1],512<<10)
        accepted=[]
        for event,fields in [('UserPromptSubmit',dict(prompt='Private current writer')),('PreToolUse',dict(tool_name='Bash',tool_use_id='incremental-private',tool_input={'command':'printf fixture'})),('Stop',dict(last_assistant_message='Private settled reply'))]:
            self.send_event(name,event,**fields) # genuine native-producer ancestry
            self.wait(lambda:status().get('state')=='ready' and status().get('generation',0)>previous)
            current=status()['generation'];self.assertGreater(current,previous);previous=current
            self.assertTrue(worker_leased(self.state),'worker died before the full listing')
            expected={str(path):path.read_bytes() for _,path in sources}
            result=subprocess.run([str(HGS),'ls','--json','--local','--no-archives'],env=self.env,capture_output=True,text=True,timeout=12)
            self.assertEqual(result.returncode,0,result.stderr);body=json.loads(result.stdout);rows=body['sessions']
            self.assertEqual(len(rows),213);self.assertEqual({row['name'] for row in rows},{record['name'] for record,_ in sources})
            native=self.binding(name);row=next(row for row in rows if row['name']==name)
            for key in ['session_id','run_id','conversation_id']:self.assertEqual(row[key],native[key])
            oracle=subprocess.run([str(HGS),'__state','merge'],input=result.stdout,env=self.env,capture_output=True,text=True,timeout=12)
            self.assertEqual(oracle.returncode,0,oracle.stderr);self.assert_rows(json.loads(oracle.stdout)['sessions'],rows)
            self.assertEqual({str(path):path.read_bytes() for _,path in sources},expected,'readiness replayed a canonical write')
            self.assertTrue(worker_leased(self.state),'worker died after the full listing')
            accepted.append(dict(generation=current,body_bytes=len(result.stdout.encode())))
        print(json.dumps(dict(control='resident-current-hook-writer',sessions=213,seed_aggregate=aggregate,cuts=accepted)),flush=True)

    def test_idle_claude_native_limit_panel_keeps_the_canonical_full_row(self):
        frame=(Path(__file__).parent/'fixtures/claude-usage-limit.txt').read_text()
        source=self.bin/'claude';script=source.read_text()
        source.write_text(script.replace('\nfor line in sys.stdin:',f'\nimport tty\ntty.setraw(sys.stdin.fileno())\nprint({chr(27)+"[?2004h"+frame.replace(chr(10),chr(13)+chr(10))!r},flush=True)\nfor line in sys.stdin:',1))
        name=self.start('claude','idle-native-limit')
        self.send_event(name,'Stop',last_assistant_message='Private fixture reply')
        record=self.binding(name)
        self.assertEqual(record['phase'],'idle')
        self.hgs('listing-index','enable','--json')
        rows=self.ready_rows()
        oracle=subprocess.run([str(HGS),'__state','merge'],input=json.dumps(dict(host='test',sessions=rows)),env=self.env,capture_output=True,text=True,timeout=12)
        self.assertEqual(oracle.returncode,0,oracle.stderr)
        expected=json.loads(oracle.stdout)['sessions']
        native=next(row for row in expected if row['name']==name)
        self.assertEqual(native['activity'],'busy')
        detail=json.loads(self.hgs('inspect',name,'--skip-processes'))
        self.assertTrue(any(q.get('source')=='claude_usage_limit' for q in detail['pending_questions']))
        self.assert_rows(expected,rows)
        self.assertEqual(self.binding(name),record,'public observation changed canonical record')

    def test_full_mobile_mixed_private_inventory_keeps_exact_canonical_detail(self):
        import threading
        frame='› \n\n  gpt-6 high · /srv/example\n'
        source=self.bin/'codex';script=source.read_text()
        source.write_text(script.replace('\nfor line in sys.stdin:',f'\nprint({frame!r},flush=True)\nfor line in sys.stdin:',1))
        owners=[self.start('kimi' if n<13 else ('codex' if n<84 else 'claude'),f'mixed-mobile-{n}') for n in range(94)]
        for n,name in enumerate(owners):
            path=self.root/'state'/(hashlib.sha256(name.encode()).hexdigest()+'.json')
            record=json.loads(path.read_text());record['fixture_padding']='x'*(72<<10)
            phase='tool' if n<13 else 'working' if n<21 else 'idle' if n<75 else 'unknown' if n<85 else 'error' if n<93 else 'compacting'
            if n<78:record['recovery_user_text']='Private fixture request '+('r'*1570)
            if n<86:record['prompt']='Private prompt '+('p'*1230)
            if n<92:record['last_message']='Private response '+('m'*610)
            record.update(phase=phase,activity='busy' if phase in ['tool','working','compacting'] else 'idle' if phase=='idle' else 'unknown')
            if phase=='error':record.update(error='Private fixture native error',attention_id=f'error-{n}',attention_started=123)
            if phase=='unknown':record['expected_id']='unconfirmed-native'
            if n in [20,93]:record['subagents']={'child':dict(state='working',name='Private worker')}
            path.write_text(json.dumps(record))
        self.hgs('listing-index','enable','--json')
        self.wait(lambda:json.loads(self.hgs('listing-index','status','--json'))['state']=='ready')
        errors=[];stop=threading.Event();started=threading.Event();writes=[]
        def writer():
            try:
                for n in range(4):
                    if stop.is_set():break
                    self.send_event(owners[13],'UserPromptSubmit',prompt=f'Private mixed writer {n}')
                    writes.append(n);started.set()
                    if stop.wait(1.5):break
            except Exception as error:errors.append(repr(error));started.set()
        thread=threading.Thread(target=writer);thread.start();self.assertTrue(started.wait(8))
        attempts=[]
        try:
            deadline=time.monotonic()+12
            while time.monotonic()<deadline:
                began=time.monotonic()
                result,resources=self.measured_full()
                attempts.append(dict(resources=resources,code=result.returncode,seconds=time.monotonic()-began,bytes=len(result.stdout.encode()),error=result.stderr.strip(),writer_alive=thread.is_alive()))
                if result.returncode==0 or 'overlays exceeded their' in result.stderr:break
                self.assertEqual(result.stdout,'');time.sleep(.05)
        finally:stop.set();thread.join(10)
        print(json.dumps(dict(control='root-mix94',writes=writes,attempts=attempts)),flush=True)
        self.assertFalse(errors,errors);self.assertFalse(thread.is_alive());self.assertEqual(result.returncode,0,result.stderr)
        self.assertTrue(attempts[-1]['writer_alive'])
        body=json.loads(result.stdout);rows=body['sessions']
        self.assertEqual(len(rows),len(owners));self.assertEqual({row['name'] for row in rows},set(owners))
        before={name:self.binding(name) for name in owners}
        for row in rows:
            for key in ['session_id','run_id','conversation_id']:self.assertEqual(row[key],before[row['name']][key])
            self.assertNotIn('listing_details',row,'withdrawn inert representation escaped')
        oracle=subprocess.run([str(HGS),'__state','merge'],input=json.dumps(body),env=self.env,capture_output=True,text=True,timeout=12)
        self.assertEqual(oracle.returncode,0,oracle.stderr)
        self.assert_rows(json.loads(oracle.stdout)['sessions'],rows)
        self.assertEqual({name:self.binding(name) for name in owners},before,'listing changed canonical records')
        target=owners[-1];detail=json.loads(self.hgs('inspect',target,'--skip-processes'))
        for key in ['name','run_id','conversation_id']:self.assertEqual(detail[key],before[target][key])
        capture=os.environ.get('ZERUS_LISTING_MIXED_CAPTURE')
        if capture:Path(capture).write_text(result.stdout)

    def test_full_mobile_snapshot_supports_live_inventory_without_whole_record_overlays(self):
        import threading
        count=int(os.environ.get('ZERUS_LISTING_LIVE_COUNT','20'))
        # Recorded ordinary Claude frame, emitted by our genuine fixture process.
        idle=os.environ.get('ZERUS_LISTING_IDLE')=='1'
        frame=(Path(__file__).parent/'fixtures'/('claude-approval-denied.txt' if idle else 'claude-approval-running.txt')).read_text()
        if idle:
            # The recorded idle pane is120 columns; emit its complete rules
            # at this private80-column terminal width, never a wrapped panel.
            frame='\n'.join('─'*40 if line and set(line)=={'─'} else line.lstrip() if line.strip().startswith('● high') else line for line in frame.splitlines())+'\n'
        source=self.bin/'claude';script=source.read_text()
        self.assertIn('\nfor line in sys.stdin:',script)
        source.write_text(script.replace('\nfor line in sys.stdin:',f'\nprint({frame!r},flush=True)\nfor line in sys.stdin:',1))
        owners=[self.start('claude',f'full-mobile-{n}') for n in range(count)]
        for name in owners:
            self.send_event(name,'UserPromptSubmit',prompt='Private live working fixture')
            self.send_event(name,'PreToolUse',tool_name='Bash',tool_use_id='private-working',tool_input={'command':'printf example'})
            if idle:self.send_event(name,'Stop',last_assistant_message='Private idle reply')
        state=self.root/'state'
        for name in owners:
            path=state/(hashlib.sha256(name.encode()).hexdigest()+'.json')
            value=json.loads(path.read_text());value['fixture_padding']='x'*int(os.environ.get('ZERUS_LISTING_PADDING_BYTES',str(256<<10)))
            path.write_text(json.dumps(value))
        self.hgs('listing-index','enable','--json')
        self.wait(lambda:json.loads(self.hgs('listing-index','status','--json'))['state']=='ready')
        errors=[];stop=threading.Event();started=threading.Event();writes=[]
        def writer():
            try:
                for n in range(4):
                    if stop.is_set():break
                    self.send_event(owners[0],'UserPromptSubmit',prompt=f'Private bounded writer {n}')
                    writes.append(n);started.set()
                    if stop.wait(1.5):break
            except Exception as error:errors.append(repr(error));started.set()
        thread=threading.Thread(target=writer);thread.start();self.assertTrue(started.wait(8))
        attempts=[];result=None
        try:
            deadline=time.monotonic()+12
            while time.monotonic()<deadline:
                began=time.monotonic()
                result,resources=self.measured_full()
                attempts.append(dict(resources=resources,code=result.returncode,seconds=time.monotonic()-began,stdout_bytes=len(result.stdout.encode()),error=result.stderr.strip(),writer_alive=thread.is_alive()))
                if result.returncode==0:break
                self.assertEqual(result.stdout,'')
                if 'overlays exceeded their' in result.stderr:break
                time.sleep(.05)
        finally:
            stop.set();thread.join(10)
        print(json.dumps(dict(control='full-mobile-live-writer',owners=count,record_bytes=sum(p.stat().st_size for p in state.glob('*.json') if len(p.stem)==64),writes=writes,attempts=attempts)),flush=True)
        self.assertFalse(errors,errors);self.assertFalse(thread.is_alive())
        self.assertEqual(result.returncode,0,result.stderr)
        self.assertTrue(attempts[-1]['writer_alive'],'no supported full cut while the bounded writer was resident')
        body=json.loads(result.stdout);rows=body['sessions']
        self.assertEqual({row['name'] for row in rows},set(owners));self.assertEqual(len(rows),count)
        for row in rows:
            record=self.binding(row['name'])
            for key in ('session_id','run_id','conversation_id'):
                self.assertEqual(row[key],record[key],(row['name'],key))
            self.assertTrue(row['tracked']);self.assertTrue(row['terminal_supported'])
        before={name:self.binding(name) for name in owners}
        oracle=subprocess.run([str(HGS),'__state','merge'],input=json.dumps(body),env=self.env,capture_output=True,text=True,timeout=12)
        self.assertEqual(oracle.returncode,0,oracle.stderr)
        self.assert_rows(json.loads(oracle.stdout)['sessions'],rows)
        self.assertEqual({name:self.binding(name) for name in owners},before,'display projection rewrote canonical bindings')
        # The encrypted native connector fixture consumes this exact CLI body.
        capture=os.environ.get('ZERUS_LISTING_CAPTURE')
        if capture:Path(capture).write_text(result.stdout)

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
