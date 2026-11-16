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
        self.env = {k:v for k,v in os.environ.items() if not k.startswith('HGS_') and k not in ('TMUX','TMUX_PANE')}
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
                if isinstance(value,dict):return {k:remove_clock(v) for k,v in value.items() if k!='sampled_at'}
                if isinstance(value,list):return [remove_clock(v) for v in value]
                return value
            return {row['name']:remove_clock(row) for row in rows}
        a,b=stable(expected),stable(actual)
        self.assertEqual(set(a),set(b))
        differences={name:[key for key in set(a[name])|set(b[name]) if a[name].get(key)!=b[name].get(key)] for name in a if a[name]!=b[name]}
        self.assertFalse(differences,f'public rows differ: {differences}')

    def test_unavailable_native_and_terminal_rows_match_the_canonical_projection(self):
        self.saved(1)
        self.native('dsh',id_layout=True)
        self.native('opencode')
        expected=self.canonical()
        self.ready()
        self.assert_rows(expected,self.complete()['sessions'])

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
        reply=self.call('ls','--json','--local','--no-archives')
        self.assertNotEqual(reply.returncode,0,'enabled corrupt index used a whole-record rescue')
        self.assertEqual(reply.stdout,'')
        self.assertEqual(path.read_bytes(),original)

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


if __name__ == '__main__':
    unittest.main()
