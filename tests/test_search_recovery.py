"""Search preparation continues after a private foreground request has exited."""
from contextlib import closing
import hashlib
import json
import os
from pathlib import Path
import sqlite3
import subprocess
import tempfile
import time
import unittest
import uuid

HGS = Path(os.environ.get('HGS_TEST_BIN', Path(__file__).resolve().parents[1] / 'target/debug/hgs')).resolve()

class SearchRecovery(unittest.TestCase):
    def test_indexed_reply_reuses_prepared_large_metadata_with_its_index_open(self):
        with tempfile.TemporaryDirectory(prefix='hgs-search-large-') as temporary:
            root=Path(temporary);state=root/'sessions';state.mkdir(mode=0o700)
            env={key:value for key,value in os.environ.items() if not key.startswith('HGS_')}
            env.update(HOME=str(root),HGS_STATE_DIR=str(state),HGS_CONFIG_DIR=str(root/'config'))
            name='codex/example/large'
            conversation=str(uuid.uuid4())
            transcript=root/(conversation+'.jsonl')
            transcript.write_text(json.dumps(dict(type='event_msg',timestamp=1,payload=dict(type='agent_message',id='public',message='public needle',phase='final')))+'\n')
            record=dict(version=1,agent='codex',name=name,run_id='run',conversation_id=conversation,transcript=str(transcript),updated=1,shell_jobs=dict(job=dict(output='x'*(9 << 20))))
            path=state/(hashlib.sha256(name.encode()).hexdigest()+'.json')
            path.write_text(json.dumps(record))
            def query(indexed):
                args=[str(HGS),'search','--query','needle','--limit','20']+(['--indexed'] if indexed else [])
                result=subprocess.run(args,env=env,text=True,capture_output=True,timeout=10)
                self.assertEqual(result.returncode,0,result.stderr)
                return json.loads(result.stdout)
            plain=query(False)
            self.assertEqual(len(plain['results']),1)
            indexed=query(True)
            self.assertEqual(len(indexed['results']),1,json.dumps(indexed))
            self.assertTrue(indexed['fallback']['discovery']['complete'])
            self.assertEqual(indexed['fallback']['discovery']['metadata_io'],0)

    def test_one_search_finishes_its_pending_catalog_without_more_search_requests(self):
        with tempfile.TemporaryDirectory(prefix='hgs-search-recovery-') as temporary:
            root = Path(temporary)
            state = root / 'state'
            state.mkdir(mode=0o700)
            for folder in ('config', 'tmp', 'tmux'):
                (root / folder).mkdir(mode=0o700)
            env = {key:value for key,value in os.environ.items() if not key.startswith('HGS_') and key not in ('TMUX','TMUX_PANE')}
            env.update(HOME=str(root), HGS_STATE_DIR=str(state), HGS_CONFIG_DIR=str(root/'config'), TMPDIR=str(root/'tmp'), TMUX_TMPDIR=str(root/'tmux'))
            (state/'sessions').mkdir()
            for n in range(800):
                conversation=str(uuid.UUID(int=n+1))
                transcript=root/(conversation+'.jsonl')
                transcript.write_text(json.dumps(dict(type='event_msg',timestamp=1,payload=dict(type='agent_message',id='public',message='public needle',phase='final')))+'\n')
                name=f'codex/example/search-{n:04}'
                record=dict(version=1,agent='codex',name=name,run_id=f'run-{n}',conversation_id=conversation,transcript=str(transcript),updated=n)
                if n >= 672:
                    record.update(layout='id',session_id=conversation)
                    path=state/'sessions'/(conversation+'.json')
                else:
                    path=state/(hashlib.sha256(name.encode()).hexdigest()+'.json')
                path.write_text(json.dumps(record))
            result=subprocess.run([str(HGS),'search','--query','needle','--limit','20','--indexed'],env=env,text=True,capture_output=True,timeout=10)
            self.assertEqual(result.returncode,0,result.stderr)
            # Read the private SQLite file directly. These observations neither
            # request another slice nor start a second helper.
            until=time.monotonic()+25
            last=None
            while time.monotonic()<until:
                try:
                    with closing(sqlite3.connect('file:'+str(state/'content-search-v1/index.sqlite3')+'?mode=ro',uri=True,timeout=.1)) as db:
                        last=db.execute('SELECT paths,pending FROM catalog_totals WHERE id=1').fetchone()
                        known=db.execute('SELECT COUNT(*) FROM catalog_queue WHERE identity IS NULL').fetchone()[0]
                        messages=db.execute('SELECT messages FROM totals WHERE id=1').fetchone()[0]
                        if last==(800,0) and known==0 and messages==800:
                            return
                except sqlite3.Error:
                    pass
                time.sleep(.1)
            self.fail(f'one foreground request abandoned its pending catalog: {last}')

if __name__=='__main__':
    unittest.main()
