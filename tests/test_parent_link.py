"""Parent links between independent sessions (zerus-sa9qn.2), in private state without real agents."""
import hashlib
from contextlib import contextmanager
import json
import os
import re
import socket
import stat
from pathlib import Path
import subprocess
import tempfile
import threading
import time
import unittest

HGS = Path(os.environ.get('HGS_TEST_BIN', Path(__file__).resolve().parents[1] / 'target/debug/hgs')).resolve()
LEAD_ID = '22222222-2222-4222-8222-222222222222'
WORKER_ID = '33333333-3333-4333-8333-333333333333'


@unittest.skipIf(os.name == 'nt', 'POSIX native-process ancestry fixture')
class ParentLink(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix='zerus-parent-')
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name).resolve()
        self.state = self.root / 'state'; self.state.mkdir()
        self.home = self.root / 'home'; self.home.mkdir()
        sockets = self.root / 'sockets'; sockets.mkdir()
        self.env = {k: v for k, v in os.environ.items() if not k.startswith('HGS_')}
        self.env.update(HOME=str(self.home), USERPROFILE=str(self.home), TMUX_TMPDIR=str(sockets),
                        HGS_CONFIG_DIR=str(self.root / 'config'), HGS_STATE_DIR=str(self.state),
                        HGS_SELF='fixture', HGS_PEERS='')
        (self.state / 'local-sessions').touch()
        start = subprocess.check_output(['ps', '-p', str(os.getpid()), '-o', 'lstart='],
                                        env=dict(self.env, LC_ALL='C', TZ='UTC'), text=True).strip()
        # The lead is owned by this test process, so hgs proves it as the caller.
        self.lead = dict(version=1, name='claude/example/lead', agent='claude', run_id='run-lead',
                         conversation_id='conversation-lead', session_id=LEAD_ID, account_id='fixture',
                         subagents={}, pid=os.getpid(), process_start=start)
        self.worker = dict(version=1, name='codex/example/worker', agent='codex', run_id='run-worker',
                           conversation_id='conversation-worker', session_id=WORKER_ID, account_id='fixture',
                           subagents={}, pid=1, process_start='not this process')
        self.write(self.lead)
        self.write(self.worker)
        # The shared catalog holds this computer's identity; links only read it.
        result = subprocess.run([str(HGS), 'reference', self.lead['name'], '--json'], env=self.env,
                                capture_output=True, text=True, timeout=10)
        self.assertEqual(result.returncode, 0, result.stderr)

    def write(self, record):
        path = self.path(record)
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(json.dumps(record))
        return path

    def path(self, record):
        directory = self.state
        if record.get('backend') in ('dsh', 'opencode'):
            directory = directory / record['backend']
            if record.get('layout') == 'id':
                return directory / 'sessions' / (record['session_id'] + '.json')
            directory = directory / 'bindings'
        return directory / (hashlib.sha256(record['name'].encode()).hexdigest() + '.json')

    def read(self, record):
        return json.loads(self.path(record).read_text())

    def native(self, agent, number=1, **extra):
        record = dict(backend=agent, agent=agent, name=f'{agent}/example/native{number}',
                      session_id=f'77777777-7777-4777-8777-{number:012d}', run_id=f'run-{agent}-{number}',
                      conversation_id=f'session-native{number}' if agent == 'dsh' else f'ses_native{number}',
                      cwd=str(self.root), agent_home=str(self.home / agent), **extra)
        self.write(record)
        return record

    @contextmanager
    def native_host(self, record):
        native_home = Path(record['agent_home'])
        native_home.mkdir(parents=True, exist_ok=True)
        host = self.state / 'dsh/hosts' / hashlib.sha256(str(native_home).encode()).hexdigest()[:16]
        host.mkdir(parents=True)
        server = socket.socket(socket.AF_UNIX)
        server.bind(str(host / 'host.sock')); server.listen(); server.settimeout(.1)
        stopped = threading.Event()
        rows = {record['conversation_id']: True}
        requests = []
        def serve():
            while not stopped.is_set():
                try:
                    client, _ = server.accept()
                except socket.timeout:
                    continue
                with client:
                    line = client.makefile().readline()
                    if not line:
                        continue
                    request = json.loads(line); requests.append(request)
                    value = {'version':'0.2.1-rc.2','protocol':1,'generation':'private-claims','accountPermissions':True}
                    if request['method'] == 'list':
                        value = {'generation':'private-claims','items':[
                            {'sessionId':sid,'agentAvailable':live,'running':False} for sid,live in rows.items()]}
                    elif request['method'] == 'create':
                        rows[request['params']['sessionId']] = True
                    client.sendall((json.dumps({'ok':True,'value':value})+'\n').encode())
        thread = threading.Thread(target=serve); thread.start()
        env = dict(self.env, DSH_HOME=str(native_home), DSH_SESSION_ID=record['conversation_id'])
        env.pop('OPENCODE_SESSION_ID', None)
        try:
            yield env, rows, requests
        finally:
            stopped.set(); thread.join(timeout=3); server.close()
            self.assertFalse(thread.is_alive())

    def test_native_self_is_claimed_and_never_becomes_process_proof(self):
        parent = self.native('dsh', 40)
        with self.native_host(parent) as (env, rows, _):
            def current(env=env, success=True):
                result = subprocess.run([str(HGS),'self','--json'], env=env, capture_output=True, text=True, timeout=10)
                self.assertEqual(result.returncode == 0, success, result.stdout+result.stderr)
                return json.loads(result.stdout) if success else result.stderr
            identity = current()
            self.assertEqual(identity['identity_source'], 'claimed')
            self.assertEqual(identity['session_id'], parent['session_id'])
            self.assertEqual(identity['parent_identity'], {k:parent[k] for k in ('session_id','name','run_id','conversation_id')})
            # A warm indexed lookup must not scan unrelated bad metadata.
            damaged = self.path(dict(parent,name='dsh/example/unrelated'))
            damaged.write_text('{broken')
            self.assertEqual(current()['session_id'], parent['session_id'])
            damaged.unlink()
            cache = next(p for p in (self.state/'native-identities').glob('*.json')
                         if parent['session_id'] in p.read_text())
            cached = cache.read_bytes(); cache.unlink(); os.mkfifo(cache,0o600)
            started = time.monotonic()
            self.assertEqual(current()['session_id'],parent['session_id'])
            self.assertLess(time.monotonic()-started,5)
            self.assertTrue(stat.S_ISFIFO(cache.lstat().st_mode))
            cache.unlink(); cache.write_bytes(cached)
            adopted = self.parent(self.worker, 'adopt', parent=identity['parent_identity'])
            self.assertEqual(adopted['parent']['source'], 'claimed')
            self.assertEqual(self.listed()[self.worker['name']]['parent']['state'], 'live')
            # The claimed environment cannot satisfy default adopt's caller proof.
            result = subprocess.run([str(HGS),'parent',self.worker['name'],'--json'], env=env,
                input=json.dumps(dict(expected_run_id=self.worker['run_id'], expected_conversation_id=self.worker['conversation_id'], action='adopt')),
                capture_output=True,text=True,timeout=10)
            self.assertNotEqual(result.returncode,0)
            self.assertIn('no live Zerus session',result.stderr)
            self.assertEqual(current(dict(env,HGS_SESSION=self.lead['name'],HGS_RUN_ID=self.lead['run_id']))['identity_source'],'process')
            self.assertIn('Multiple native',current(dict(env,OPENCODE_SESSION_ID='ses_other'),success=False))
            rows[parent['conversation_id']] = False
            self.assertIn('live session',current(success=False))

    def test_claimed_adoption_requires_complete_fresh_live_identity_and_refuses_cycles(self):
        parent = self.native('dsh', 41)
        choice = {k:parent[k] for k in ('session_id','name','run_id','conversation_id')}
        with self.native_host(parent) as (_, rows, _):
            without_name = {k:v for k,v in choice.items() if k!='name'}
            self.assertEqual(self.parent(self.worker,'adopt',parent=without_name)['parent']['source'],'claimed')
            self.parent(self.worker,'clear')
            for changed in (dict(choice,run_id='old'),dict(choice,conversation_id='session-old')):
                self.assertIn('changed',self.parent(self.worker,'adopt',parent=changed,success=False))
            for field in ('session_id','run_id','conversation_id'):
                self.assertIn('exact',self.parent(self.worker,'adopt',parent=dict(choice,**{field:''}),success=False))
            self.assertIn('own parent',self.parent(parent,'adopt',parent=choice,success=False))
            self.choose(parent,self.worker)
            self.assertIn('cycle',self.parent(self.worker,'adopt',parent=choice,success=False))
            self.parent(parent,'clear')
            rows[parent['conversation_id']] = False
            self.assertIn('live session',self.parent(self.worker,'adopt',parent=choice,success=False))
            rows[parent['conversation_id']] = True
            self.write(dict(parent,state='archived'))
            self.assertIn('live session',self.parent(self.worker,'adopt',parent=choice,success=False))

    def test_explicit_launch_parent_is_claimed_and_never_changes_an_existing_session(self):
        parent = self.native('dsh', 42)
        choice = json.dumps({k:parent[k] for k in ('session_id','name','run_id','conversation_id')})
        with self.native_host(parent) as (env, _, requests):
            for agent in ('claude','codex','kimi','dsh','opencode'):
                result = subprocess.run([str(HGS),'--dry-run',agent,str(self.root),'--new','-n','claim','-d','--parent',choice],
                    env=env,capture_output=True,text=True,timeout=10)
                self.assertEqual(result.returncode,0,result.stderr)
            for agent in ('claude','dsh','opencode'):
                result = subprocess.run([str(HGS),'--dry-run','@peer',agent,'/work/example','--new','-n','claim','-d','--parent',choice],
                    env=dict(env,HGS_PEERS='peer'),capture_output=True,text=True,timeout=10)
                self.assertEqual(result.returncode,0,result.stderr)
                self.assertIn('claimed-parent-v1',result.stdout)
            args = [str(HGS),'dsh',str(self.root),'-n','claimed-child','-d','--parent',choice]
            result = subprocess.run([*args,'--new'],env=env,capture_output=True,text=True,timeout=10)
            self.assertEqual(result.returncode,0,result.stderr)
            child = next(json.loads(p.read_text()) for p in (self.state/'dsh/bindings').glob('*.json')
                         if json.loads(p.read_text())['name'].endswith('/claimed-child'))
            self.assertEqual((child['parent']['source'],child['parent']['session_id']),('claimed',parent['session_id']))
            before = len([r for r in requests if r['method']=='create'])
            refused = subprocess.run(args,env=env,capture_output=True,text=True,timeout=10)
            self.assertNotEqual(refused.returncode,0)
            self.assertIn('only to new sessions',refused.stderr)
            self.assertEqual(before,len([r for r in requests if r['method']=='create']))

    def parent(self, target, action, caller=None, success=True, **extra):
        env = dict(self.env)
        if caller:
            env.update(HGS_SESSION=caller['name'], HGS_RUN_ID=caller['run_id'])
        request = dict(expected_run_id=target['run_id'], expected_conversation_id=target['conversation_id'],
                       action=action, **extra)
        result = subprocess.run([str(HGS), 'parent', target['name'], '--json'], env=env, input=json.dumps(request),
                                capture_output=True, text=True, timeout=10)
        if success:
            self.assertEqual(result.returncode, 0, result.stderr)
            return json.loads(result.stdout)
        self.assertNotEqual(result.returncode, 0, result.stdout)
        return result.stderr

    def listed(self):
        result = subprocess.run([str(HGS), 'ls', '--json', '--local'], env=self.env, capture_output=True,
                                text=True, timeout=20)
        self.assertEqual(result.returncode, 0, result.stderr)
        rows = json.loads(result.stdout)
        rows = rows[0]['sessions'] if isinstance(rows, list) else rows['sessions']
        return {row['name']: row for row in rows}

    def test_an_agent_adopts_a_session_and_the_listing_reports_it(self):
        self.assertIsNone(self.parent(self.worker, 'read')['parent'])
        adopted = self.parent(self.worker, 'adopt', caller=self.lead)
        link = adopted['parent']
        self.assertEqual((link['session_id'], link['name'], link['run_id'], link['conversation_id'], link['source'], link['host']),
                         (LEAD_ID, self.lead['name'], 'run-lead', 'conversation-lead', 'adopt', 'fixture'))
        self.assertEqual(self.read(self.worker)['parent'], link)
        row = self.listed()[self.worker['name']]
        self.assertEqual(row['parent']['session_id'], LEAD_ID)
        # The lead is saved here (no live pane), never live.
        self.assertEqual(row['parent']['state'], 'saved')
        self.assertNotIn('parent', self.listed()[self.lead['name']])
        # Adopting again by the same lead is accepted and keeps one link.
        self.assertEqual(self.parent(self.worker, 'adopt', caller=self.lead)['parent']['session_id'], LEAD_ID)
        cleared = self.parent(self.worker, 'clear')
        self.assertIsNone(cleared['parent'])
        self.assertEqual(cleared['previous']['session_id'], LEAD_ID)
        self.assertNotIn('parent', self.read(self.worker))
        self.assertNotIn('parent', self.listed()[self.worker['name']])

    def test_a_held_catalog_lock_never_blocks_listing_or_adoption(self):
        # zerus-hycp0: the link reads the machine identity without the catalog
        # lock, and a listing never reads it at all.
        import fcntl
        lock = open(self.root / 'config' / 'swarm' / 'catalog.lock', 'a+b')
        self.addCleanup(lock.close)
        fcntl.flock(lock, fcntl.LOCK_EX)
        started = time.monotonic()
        self.assertEqual(self.parent(self.worker, 'adopt', caller=self.lead)['parent']['session_id'], LEAD_ID)
        self.assertEqual(self.listed()[self.worker['name']]['parent']['state'], 'saved')
        self.assertLess(time.monotonic() - started, 5)
        fcntl.flock(lock, fcntl.LOCK_UN)

    def index(self, record):
        ids = self.state / 'ids'
        ids.mkdir(mode=0o700, exist_ok=True)
        entry = ids / (record['session_id'] + '.json')
        entry.write_text(json.dumps({'version': 1, 'name': record['name'], 'kind': record.get('backend', 'record')}))
        entry.chmod(0o600)

    def link_to(self, record):
        return dict(schema=1, machine=self.read_link()['machine'], host='fixture', session_id=record['session_id'],
                    name=record['name'], conversation_id=record['conversation_id'], run_id=record['run_id'],
                    source='adopt', at=1)

    def read_link(self):
        if not hasattr(self, '_link'):
            self._link = self.parent(self.worker, 'adopt', caller=self.lead)['parent']
            self.parent(self.worker, 'clear')
        return self._link

    def test_a_listing_never_creates_or_saves_the_shared_catalog(self):
        self.parent(self.worker, 'adopt', caller=self.lead)
        catalog = self.root / 'config' / 'swarm'
        for entry in catalog.iterdir():
            entry.unlink()
        catalog.rmdir()
        self.assertEqual(self.listed()[self.worker['name']]['parent']['state'], 'saved')
        self.assertFalse(catalog.exists())
        # Without a readable identity, adoption asks to retry instead of creating one.
        self.assertIn('identity cannot be read', self.parent(self.worker, 'adopt', caller=self.lead, success=False))
        self.assertFalse(catalog.exists())

    def test_a_cycle_beyond_eight_links_is_refused_not_assumed_absent(self):
        # zerus-lwz9t (review-win-c control): lead -> a1 -> ... -> a9 -> worker.
        chain = [dict(self.lead)] + [dict(self.worker, name=f'claude/example/ancestor{i}',
                                          session_id=f'88888888-8888-4888-8888-{i:012d}', run_id=f'run-ancestor{i}',
                                          conversation_id=f'conv-ancestor{i}') for i in range(1, 10)] + [dict(self.worker)]
        link = self.read_link()
        for a, b in zip(chain, chain[1:]):
            a['parent'] = dict(link, session_id=b['session_id'], name=b['name'], run_id=b['run_id'],
                               conversation_id=b['conversation_id'])
            self.write(a)
            self.index(a)
        refusal = self.parent(self.worker, 'adopt', caller=chain[0], success=False)
        self.assertIn('deeper than 8', refusal)
        self.assertNotIn('parent', self.read(self.worker))
        # The index is only a cache: without the first entry the walk reads the
        # labelled records and still refuses (zerus-lwz9t).
        (self.state / 'ids' / (chain[1]['session_id'] + '.json')).unlink()
        self.assertIn('deeper than 8', self.parent(self.worker, 'adopt', caller=chain[0], success=False))
        for entry in (self.state / 'ids').iterdir():
            entry.unlink()
        self.assertIn('deeper than 8', self.parent(self.worker, 'adopt', caller=chain[0], success=False))
        self.assertNotIn('parent', self.read(self.worker))

    def test_an_ancestor_record_that_is_a_fifo_never_blocks_adoption(self):
        # zerus-mzgfh (review-win-c control): an indexed ancestor replaced by a FIFO.
        other = dict(self.worker, name='claude/example/ancestor', session_id='99999999-9999-4999-8999-999999999999',
                     run_id='run-ancestor', conversation_id='conversation-ancestor')
        path = self.write(other)
        self.index(other)
        lead = dict(self.lead, parent=self.link_to(other))
        self.write(lead)
        path.unlink()
        os.mkfifo(path, 0o600)
        started = time.monotonic()
        refusal = self.parent(self.worker, 'adopt', caller=lead, success=False, replace=True)
        self.assertLess(time.monotonic() - started, 5)
        self.assertIn('cannot be read safely', refusal)
        # The state writer is free again.
        import fcntl
        with open(self.state / '.lock', 'a+b') as lock:
            fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)

    def choose(self, target, parent, success=True, **extra):
        choice = dict(session_id=parent['session_id'], name=parent['name'], conversation_id=parent['conversation_id'],
                      run_id=parent['run_id'])
        return self.parent(target, 'set', success=success, parent=choice, **extra)

    def test_a_person_sets_an_explicit_parent_on_this_computer(self):
        # zerus-sa9qn.9: the desktop names the parent; no calling agent is needed.
        link = self.choose(self.worker, self.lead)['parent']
        self.assertEqual((link['session_id'], link['name'], link['source']), (LEAD_ID, self.lead['name'], 'manual'))
        self.assertEqual(self.listed()[self.worker['name']]['parent']['session_id'], LEAD_ID)
        # Changing it to another session needs replace; the same parent does not.
        other = dict(self.lead, name='claude/example/other', run_id='run-other', conversation_id='conversation-other',
                     session_id='44444444-4444-4444-8444-444444444444', pid=1, process_start='elsewhere')
        self.write(other)
        self.assertEqual(self.choose(self.worker, self.lead)['parent']['session_id'], LEAD_ID)
        self.assertIn('already has the parent', self.choose(self.worker, other, success=False))
        self.assertEqual(self.choose(self.worker, other, replace=True)['previous']['session_id'], LEAD_ID)

    def test_an_explicit_parent_must_be_exact_and_never_close_a_loop(self):
        self.assertIn('own parent', self.choose(self.lead, self.lead, success=False))
        self.assertIn('changed', self.choose(self.worker, dict(self.lead, run_id='run-old'), success=False))
        gone = dict(self.lead, session_id='55555555-5555-4555-8555-555555555555', name='claude/example/gone')
        self.assertIn('not a session on this computer', self.choose(self.worker, gone, success=False))
        self.parent(self.worker, 'set', success=False)  # set needs a parent
        self.parent(self.worker, 'clear', success=False, parent=dict(session_id=LEAD_ID))  # only set names one
        # worker leads lead; making worker a child of lead would close the loop.
        self.choose(self.lead, self.worker)
        self.assertIn('cycle', self.choose(self.worker, self.lead, success=False))
        self.assertNotIn('parent', self.read(self.worker))

    def test_native_children_read_adopt_set_clear_and_exact_guards(self):
        for agent in ('dsh', 'opencode'):
            with self.subTest(agent=agent):
                child = self.native(agent)
                self.assertIsNone(self.parent(child, 'read')['parent'])
                adopted = self.parent(child, 'adopt', caller=self.lead)['parent']
                self.assertEqual(adopted['session_id'], LEAD_ID)
                self.assertEqual(self.read(child)['parent'], adopted)
                row = self.listed()[child['name']]
                self.assertEqual(row['parent']['session_id'], LEAD_ID)
                self.assertEqual(row['parent']['state'], 'saved')
                for stale in (dict(child, run_id='old-run'), dict(child, conversation_id='old-conversation')):
                    self.assertIn('changed', self.parent(stale, 'clear', success=False))
                self.assertEqual(self.parent(child, 'read')['parent'], adopted)
                self.assertIn('already has the parent', self.choose(child, self.worker, success=False))
                self.assertEqual(self.choose(child, self.worker, replace=True)['previous'], adopted)
                self.assertEqual(self.parent(child, 'clear')['previous']['session_id'], WORKER_ID)
                self.assertNotIn('parent', self.read(child))
                self.write(dict(child, state='archived'))
                for action in ('read', 'adopt', 'clear'):
                    self.assertIn('Archived sessions', self.parent(child, action, caller=self.lead, success=False))

    def test_native_parents_in_both_directions_keep_cycles_and_caller_proof(self):
        dsh = self.native('dsh', 1)
        opencode = self.native('opencode', 2)
        self.index(dsh)
        self.index(opencode)
        self.assertEqual(self.choose(self.worker, dsh)['parent']['conversation_id'], dsh['conversation_id'])
        self.assertEqual(self.choose(dsh, opencode)['parent']['session_id'], opencode['session_id'])
        self.assertIn('cycle', self.choose(opencode, self.worker, success=False))
        # Dropping the derived index cannot hide native ancestors.
        for record in (dsh, opencode):
            (self.state / 'ids' / (record['session_id'] + '.json')).unlink()
        self.assertIn('cycle', self.choose(opencode, self.worker, success=False))
        for record in (dsh, opencode):
            self.assertIn('no live Zerus session', self.parent(self.lead, 'adopt', caller=record, success=False))
            self.assertIn('changed', self.choose(self.lead, dict(record, conversation_id='stale'), success=False))
            self.assertIn('own parent', self.choose(record, record, success=False))
        self.parent(dsh, 'clear')
        self.write(dict(opencode, state='archived'))
        self.assertIn('archived session cannot be a parent', self.choose(self.lead, opencode, success=False))

    def test_native_chain_remains_bounded_and_unsafe_records_refuse(self):
        chain = [self.native('dsh' if i % 2 else 'opencode', i) for i in range(1, 11)]
        for child, parent in zip(chain, chain[1:]):
            self.write(dict(child, parent=self.link_to(parent)))
            self.index(child)
        self.assertIn('deeper than 8', self.choose(self.worker, chain[0], success=False))
        for record in chain:
            entry = self.state / 'ids' / (record['session_id'] + '.json')
            if entry.exists():
                entry.unlink()
        self.assertIn('deeper than 8', self.choose(self.worker, chain[0], success=False))
        # A native FIFO or symlink must refuse promptly, with or without an index.
        other = self.native('dsh', 20)
        parent = self.native('opencode', 21, parent=self.link_to(other))
        path = self.path(other)
        path.unlink()
        os.mkfifo(path, 0o600)
        started = time.monotonic()
        self.assertIn('cannot be read safely', self.choose(self.worker, parent, success=False))
        self.assertLess(time.monotonic() - started, 5)
        path.unlink()
        path.symlink_to(self.path(self.lead))
        self.assertIn('cannot be read safely', self.choose(self.worker, parent, success=False))

    def test_native_id_layout_links_are_changed_in_place(self):
        records = [self.native('dsh', 30, layout='id'), self.native('opencode', 31, layout='id')]
        for record in records:
            labels = self.state / 'labels'
            labels.mkdir(exist_ok=True)
            digest = hashlib.sha256(record['name'].encode()).hexdigest()
            (labels / (digest + '.json')).write_text(json.dumps(dict(version=1, label=record['name'], ids=[record['session_id']])))
            (labels / '.complete').unlink(missing_ok=True)
        child, parent = records
        linked = self.choose(child, parent)['parent']
        self.assertEqual(linked['session_id'], parent['session_id'])
        self.assertEqual(self.read(child)['parent'], linked)
        self.assertEqual(self.choose(self.worker, child)['parent']['session_id'], child['session_id'])
        self.assertIn('cycle', self.choose(parent, self.worker, success=False))
        self.assertIsNone(self.parent(child, 'clear')['parent'])
        self.assertEqual(self.parent(child, 'adopt', caller=self.lead)['parent']['session_id'], LEAD_ID)
        for record in records:
            legacy = self.state / record['backend'] / 'bindings' / (hashlib.sha256(record['name'].encode()).hexdigest() + '.json')
            self.assertFalse(legacy.exists(), 'a parent edit must not recreate a label binding')

    def test_new_dsh_launch_records_proven_parent_before_create_but_resume_does_not(self):
        native_home = self.home / 'private-dsh'
        host = self.state / 'dsh/hosts' / hashlib.sha256(str(native_home).encode()).hexdigest()[:16]
        host.mkdir(parents=True)
        server = socket.socket(socket.AF_UNIX)
        server.bind(str(host / 'host.sock'))
        server.listen()
        server.settimeout(.1)
        stopped = threading.Event()
        created = []
        def serve():
            while not stopped.is_set():
                try:
                    client, _ = server.accept()
                except socket.timeout:
                    continue
                with client:
                    request = json.loads(client.makefile().readline())
                    if request['method'] == 'create':
                        # Persistence, including parent identity, precedes admission.
                        records = [json.loads(p.read_text()) for p in (self.state / 'dsh/bindings').glob('*.json')]
                        created.append(next(r for r in records if r['conversation_id'] == request['params']['sessionId']))
                    value = {'version': '0.2.1-rc.2', 'protocol': 1, 'generation': 'private-fixture'}
                    client.sendall((json.dumps({'ok': True, 'value': value}) + '\n').encode())
        thread = threading.Thread(target=serve)
        thread.start()
        try:
            env = dict(self.env, DSH_HOME=str(native_home), HGS_SESSION=self.lead['name'],
                       HGS_RUN_ID=self.lead['run_id'], HGS_PARENT=json.dumps(self.link_to(self.worker)))
            def launch(tag, env, new=False):
                result = subprocess.run([str(HGS), 'dsh', str(self.root), '-n', tag, '-d', *(['--new'] if new else [])],
                                        env=env, capture_output=True, text=True, timeout=15)
                self.assertEqual(result.returncode, 0, result.stderr)
                return next(json.loads(p.read_text()) for p in (self.state / 'dsh/bindings').glob('*.json')
                            if json.loads(p.read_text())['name'].endswith('/' + tag))
            child = launch('linked', env, new=True)
            self.assertEqual(child['parent']['session_id'], LEAD_ID)
            self.assertEqual(child['parent']['source'], 'launch')
            self.assertEqual(created[-1]['parent'], child['parent'])
            self.parent(child, 'clear')
            resumed = launch('linked', env)
            self.assertNotIn('parent', resumed)
            self.assertEqual((resumed['session_id'], resumed['conversation_id']), (child['session_id'], child['conversation_id']))
            plain = launch('plain', dict(self.env, DSH_HOME=str(native_home), HGS_PARENT=env['HGS_PARENT']), new=True)
            self.assertNotIn('parent', plain)
        finally:
            stopped.set()
            thread.join(timeout=3)
            server.close()
            self.assertFalse(thread.is_alive())

    def test_a_remote_launch_forwards_the_proven_caller_behind_the_peer_marker(self):
        # zerus-sa9qn.7: the launch on a peer carries the caller as --parent-link,
        # and only a peer that lists launch-parent-v1 receives it.
        env = dict(self.env, HGS_PEERS='peer', HGS_SESSION=self.lead['name'], HGS_RUN_ID=self.lead['run_id'])
        run = lambda env: subprocess.run([str(HGS), '--dry-run', '@peer', 'claude', '/work/example', '--new', '-n', 'kid', '-d'],
                                         env=env, capture_output=True, text=True, timeout=10)
        proven = run(env)
        self.assertEqual(proven.returncode, 0, proven.stderr)
        self.assertIn('launch-parent-v1', proven.stdout)
        hex_link = re.search(r"--parent-link\W+([0-9a-f]+)", proven.stdout).group(1)
        link = json.loads(bytes.fromhex(hex_link))
        self.assertEqual((link['session_id'], link['source'], link['name']), (LEAD_ID, 'launch', self.lead['name']))
        plain = run(dict(self.env, HGS_PEERS='peer'))
        self.assertEqual(plain.returncode, 0, plain.stderr)
        self.assertNotIn('--parent-link', plain.stdout)

    def test_native_remote_launches_forward_only_the_proven_caller(self):
        for agent in ('dsh', 'opencode'):
            env = dict(self.env, HGS_PEERS='peer', HGS_SESSION=self.lead['name'], HGS_RUN_ID=self.lead['run_id'])
            result = subprocess.run([str(HGS), '--dry-run', '@peer', agent, '/work/example', '--new', '-n', 'kid', '-d'],
                                    env=env, capture_output=True, text=True, timeout=10)
            self.assertEqual(result.returncode, 0, result.stderr)
            link = json.loads(bytes.fromhex(re.search(r'--parent-link\W+([0-9a-f]+)', result.stdout).group(1)))
            self.assertEqual((link['session_id'], link['source']), (LEAD_ID, 'launch'))

    def test_a_forwarded_parent_stays_missing_even_when_its_ids_match_here(self):
        # zerus-l4ehe: another computer's claim is never matched to a session here.
        link = self.parent(self.worker, 'adopt', caller=self.lead)['parent']
        self.assertEqual(self.listed()[self.worker['name']]['parent']['state'], 'saved')
        self.write(dict(self.read(self.worker), parent=dict(link, source='forwarded')))
        row = self.listed()[self.worker['name']]
        self.assertEqual((row['parent']['source'], row['parent']['state']), ('forwarded', 'missing'))
        # A malformed --parent-link is refused without a panic (zerus-so6hr).
        refused = subprocess.run([str(HGS), '--dry-run', 'claude', str(self.root), '--new', '-n', 'kid', '-d',
                                  '--parent-link', '\u20acx'], env=self.env, capture_output=True, text=True, timeout=10)
        self.assertEqual(refused.returncode, 1, refused.stderr)
        self.assertIn('--parent-link needs a launch link', refused.stderr)

    def test_adoption_needs_a_proven_caller_and_the_exact_target(self):
        self.assertIn('no live Zerus session', self.parent(self.worker, 'adopt', success=False))
        self.assertIn('cannot adopt itself', self.parent(self.lead, 'adopt', caller=self.lead, success=False))
        stale = dict(self.worker, run_id='run-old')
        self.assertIn('changed', self.parent(stale, 'adopt', caller=self.lead, success=False))
        self.parent(self.worker, 'remove', caller=self.lead, success=False)
        # Environment labels without the owned process prove nothing.
        self.write(dict(self.lead, process_start='not the owned process'))
        self.assertIn('no live Zerus session', self.parent(self.worker, 'adopt', caller=self.lead, success=False))
        self.assertNotIn('parent', self.read(self.worker))

    def test_another_parent_needs_replace_and_a_cycle_is_refused(self):
        other = dict(self.lead, name='claude/example/other', run_id='run-other', conversation_id='conversation-other',
                     session_id='44444444-4444-4444-8444-444444444444')
        self.write(self.lead)
        self.parent(self.worker, 'adopt', caller=self.lead)
        # The lead record stops being owned so the other lead can be the caller.
        self.write(dict(self.lead, pid=1, process_start='elsewhere'))
        self.write(other)
        self.assertIn('already has the parent', self.parent(self.worker, 'adopt', caller=other, success=False))
        self.assertEqual(self.parent(self.worker, 'adopt', caller=other, replace=True)['parent']['name'], other['name'])
        # other leads worker; worker adopting other would close a loop.
        start = other['process_start']
        self.write(dict(other, pid=1, process_start='elsewhere'))
        self.write(dict(self.read(self.worker), pid=os.getpid(), process_start=start))
        refusal = self.parent(other, 'adopt', caller=self.read(self.worker), success=False)
        self.assertIn('cycle', refusal)


if __name__ == '__main__':
    unittest.main()
