"""Real Git, isolated state: policy, periodic safety and crash/no-replay guards."""
import json
import os
from pathlib import Path
import shutil
import sqlite3
import subprocess
import sys
import time
import unittest

from test_worktrees import Repository, HGS
import test_worktrees


class Maintenance(Repository, unittest.TestCase):
    def call(self, *args, payload=None, ok=True):
        p=subprocess.run([str(HGS),'worktrees',*args],input=json.dumps(payload) if payload is not None else None,
                         env=self.env,capture_output=True,text=True,timeout=30)
        self.assertEqual(p.returncode==0,ok,p.stdout+p.stderr)
        return json.loads(p.stdout) if ok else p.stderr

    def managed(self):
        tree=self.root/'managed'
        self.call('create','--path',str(self.repo),'--destination',str(tree),'--branch','session-work','--json')
        metadata=Path(self.git('-C',str(tree),'rev-parse','--absolute-git-dir'))
        old=time.time()-3*86400
        for key in ('HEAD','index','logs/HEAD'):
            if (metadata/key).exists():os.utime(metadata/key,(old,old))
        return tree

    def save(self, **changes):
        p=self.call('policy','get','--json')['policy'];p.update(changes)
        return self.call('policy','set','--json',payload=p)['policy']

    def test_default_read_is_nonmutating_and_stale_policy_cannot_overwrite(self):
        p=self.call('policy','get','--json')['policy']
        self.assertTrue(all(p[k] for k in ('enabled','after_session','unused_integrated','missing')))
        self.assertEqual(p['grace_hours'],24)
        self.assertFalse((self.root/'state/worktree-maintenance').exists())
        self.save(enabled=False)
        self.call('policy','set','--json',payload=p,ok=False)
        self.assertFalse(self.call('policy','get','--json')['policy']['enabled'])

    def test_partial_policy_input_and_stored_policy_never_enable_default_rules(self):
        tree=self.managed()
        reason=self.call('policy','set','--json',payload={'version':1,'revision':0},ok=False)
        self.assertIn('missing field',reason)
        policy=self.root/'state/worktree-maintenance/policy.json'
        policy.write_text('{"version":1,"revision":0}')
        reason=self.call('sweep','--json',ok=False)
        self.assertIn('missing field',reason)
        self.assertTrue(tree.exists())

    def test_sweep_removes_integrated_unused_checkout_and_keeps_branch(self):
        tree=self.managed();self.save(after_session=False)
        r=self.call('sweep','--json');self.assertEqual(r['status'],'removed',r)
        self.assertFalse(tree.exists());self.assertTrue(self.git('-C',str(self.repo),'show-ref','refs/heads/session-work'))
        self.assertEqual(self.call('sweep','--json')['status'],'waiting')

    def test_each_periodic_toggle_and_master_disable_protect_checkout(self):
        for change in ({'enabled':False},{'unused_integrated':False},{'grace_hours':8760}):
            with self.subTest(change=change):
                # Separate state and repository for each policy scenario.
                self.setUp();tree=self.managed();self.save(**change)
                self.call('sweep','--json');self.assertTrue(tree.exists())

    def test_local_changes_and_ignored_files_are_preserved(self):
        for kind in ('dirty','ignored','unmerged'):
            with self.subTest(kind=kind):
                self.setUp();tree=self.managed()
                if kind=='ignored':
                    (self.repo/'.git/info/exclude').write_text('local-secret\n');(tree/'local-secret').write_text('fixture')
                elif kind=='dirty':(tree/'draft').write_text('keep')
                else:
                    (tree/'work').write_text('feature');self.git('-C',str(tree),'add','work')
                    self.git('-C',str(tree),'-c','user.name=Fixture','-c','user.email=fixture@example.test','commit','-qm','pending')
                r=self.call('sweep','--json');self.assertEqual(r['status'],'kept',r);self.assertTrue(tree.exists())

    def test_missing_nonce_and_git_lock_protect_checkout(self):
        for kind in ('nonce','lock'):
            with self.subTest(kind=kind):
                self.setUp();tree=self.managed()
                if kind=='nonce':
                    metadata=Path(self.git('-C',str(tree),'rev-parse','--absolute-git-dir'));(metadata/'zerus-owner.json').unlink()
                else:self.git('-C',str(self.repo),'worktree','lock',str(tree))
                self.assertEqual(self.call('sweep','--json')['status'],'kept');self.assertTrue(tree.exists())

    def test_missing_folder_can_be_forgotten_but_a_saved_session_protects_it(self):
        tree=self.managed();shutil.rmtree(tree)
        self.assertEqual(self.call('sweep','--json')['status'],'forgotten')
        self.assertTrue(self.git('-C',str(self.repo),'show-ref','refs/heads/session-work'))
        self.setUp();tree=self.managed();shutil.rmtree(tree)
        test_worktrees.Cleanup.binding(self,'codex/fixture/saved',tree)
        result=self.call('sweep','--json');self.assertEqual(result['status'],'kept',result)
        self.assertIn('session',result['reason'].lower())

    def test_missing_checkout_without_its_retained_branch_is_never_forgotten(self):
        tree=self.managed();self.git('-C',str(self.repo),'update-ref','-d','refs/heads/session-work');shutil.rmtree(tree)
        result=self.call('sweep','--json');self.assertEqual(result['status'],'kept',result)
        self.assertIn('retained by its branch',result['reason'])

    def test_crashed_automatic_intent_is_never_replayed(self):
        tree=self.managed();db=self.root/'state/worktree-maintenance/queue.sqlite3'
        with sqlite3.connect(db) as conn:conn.execute("UPDATE checkouts SET status='unconfirmed'")
        self.assertEqual(self.call('sweep','--json')['status'],'waiting');self.assertTrue(tree.exists())
        reason=self.call('remove','--path',str(tree),'--common-dir',str(self.repo/'.git'),
                         '--automatic','--periodic','--fingerprint','automatic','--json',ok=False)
        self.assertIn('earlier automatic attempt',reason);self.assertTrue(tree.exists())

    def test_unmanaged_worktree_is_never_swept(self):
        tree=self.linked();self.call('sweep','--json');self.assertTrue(tree.exists())

    def test_incomplete_archive_metadata_keeps_automatic_cleanup_safe(self):
        tree=self.managed();archive=self.root/'state/archives';archive.mkdir()
        (archive/'unknown.json').write_text('{broken')
        result=self.call('sweep','--json');self.assertEqual(result['status'],'kept',result);self.assertTrue(tree.exists())

    def test_dry_run_cannot_sweep_or_change_policy(self):
        tree=self.managed();policy=self.call('policy','get','--json')['policy']
        for args in (['--dry-run','worktrees','sweep','--json'],['--dry-run','worktrees','policy','set','--json']):
            p=subprocess.run([str(HGS),*args],input=json.dumps(dict(policy,enabled=False)),env=self.env,capture_output=True,text=True,timeout=10)
            self.assertNotEqual(p.returncode,0,p.stdout+p.stderr)
            self.assertTrue(tree.exists());self.assertEqual(self.call('policy','get','--json')['policy'],policy)

    @unittest.skipUnless(sys.platform == 'linux', 'Linux nondumpable process contract')
    def test_inaccessible_live_process_keeps_automatic_cleanup_from_claiming_disuse(self):
        tree=self.managed();ready=self.root/'nondumpable-ready'
        script = "import ctypes,pathlib,sys,time; assert ctypes.CDLL(None,use_errno=True).prctl(4,0,0,0,0)==0; pathlib.Path(sys.argv[1]).write_text('ready'); time.sleep(30)"
        child=subprocess.Popen([sys.executable,'-I','-B','-c',script,str(ready)],cwd=tree,env=self.env)
        try:
            end=time.monotonic()+5
            while not ready.exists() and child.poll() is None and time.monotonic()<end:time.sleep(.01)
            self.assertTrue(ready.exists(),'fixture did not establish inaccessible process metadata')
            self.assertIsNone(child.poll())
            result=self.call('sweep','--json')
            self.assertEqual(result['status'],'kept',result)
            self.assertIn('process',result['reason'].lower())
            self.assertTrue(tree.exists())
        finally:
            if child.poll() is None:child.terminate()
            child.wait(timeout=5)

    def test_executable_running_from_checkout_protects_it_with_another_cwd(self):
        if not shutil.which('sleep'):self.skipTest('sleep fixture unavailable')
        # System macOS executables carry restricted file flags. The fixture
        # needs their bytes and executable mode, not those system-only flags.
        tree=self.managed();image=tree/'fixture-sleep';shutil.copyfile(shutil.which('sleep'),image);image.chmod(0o755)
        child=subprocess.Popen([str(image),'30'],cwd=self.root,env=self.env)
        try:
            r=self.call('review','--path',str(self.repo),'--worktree',str(tree),'--json')['worktrees'][0]
            self.assertIn('processes',[r['code'] for r in r['reasons']])
        finally:child.terminate();child.wait(timeout=5)


if __name__=='__main__':unittest.main()
