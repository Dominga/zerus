"""Parent links between independent sessions (zerus-sa9qn.2), in private state without real agents."""
import hashlib
import json
import os
import re
from pathlib import Path
import subprocess
import tempfile
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
        path = self.state / (hashlib.sha256(record['name'].encode()).hexdigest() + '.json')
        path.write_text(json.dumps(record))
        return path

    def read(self, record):
        return json.loads((self.state / (hashlib.sha256(record['name'].encode()).hexdigest() + '.json')).read_text())

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
        entry.write_text(json.dumps({'version': 1, 'name': record['name'], 'kind': 'record'}))
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
