"""Parent links between independent sessions (zerus-sa9qn.2), in private state without real agents."""
import hashlib
import json
import os
from pathlib import Path
import subprocess
import tempfile
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
