#!/usr/bin/env python3
"""Catalogs and explicit worktree creation against isolated real Git repositories."""
import concurrent.futures
import contextlib
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

REPO=Path(__file__).resolve().parents[1]
HGS=Path(os.environ.get('HGS_TEST_BIN',REPO/'target/debug/hgs')).resolve()
GIT=shutil.which('git')

def codes(items):return [item['code'] for item in items]

class Repository:
    def setUp(self):
        self.temp=tempfile.TemporaryDirectory(prefix='hgs-worktrees-');self.addCleanup(self.temp.cleanup)
        self.root=Path(self.temp.name);self.home=self.root/'home';self.home.mkdir()
        self.env=dict(os.environ,HOME=str(self.home),XDG_CONFIG_HOME=str(self.home/'.config'),HGS_CONFIG_DIR=str(self.home/'.config/hgs'),HGS_STATE_DIR=str(self.root/'state'),HGS_SELF='fixture',HGS_PEERS='',GIT_CONFIG_NOSYSTEM='1',GIT_CONFIG_GLOBAL='/dev/null')
        for key in ('GIT_DIR','GIT_COMMON_DIR','GIT_WORK_TREE','GIT_INDEX_FILE','GIT_CONFIG_COUNT'):self.env.pop(key,None)
        self.repo=self.root/'проект repo';self.repo.mkdir();self.git('init','-q','-b','main',str(self.repo))
        self.git('-C',str(self.repo),'-c','user.name=Fixture','-c','user.email=fixture@example.test','commit','--allow-empty','-qm','initial')
    def git(self,*args):
        return subprocess.check_output([GIT,*args],env=self.env,stderr=subprocess.PIPE).decode().strip()
    def catalog(self,path=None,refresh=False):
        result=subprocess.run([str(HGS),'worktrees','--path',str(path or self.repo),'--json',*(['--refresh'] if refresh else [])],env=self.env,text=True,capture_output=True,timeout=7)
        self.assertEqual(result.returncode,0,result.stderr);return json.loads(result.stdout)
    def fake(self,body):
        file=self.home/'.local/bin/git';file.parent.mkdir(parents=True,exist_ok=True)
        file.write_text('#!'+sys.executable+'\nimport os,sys,time\n'+body);file.chmod(0o755)
    def linked(self,name='linked tree'):
        path=self.root/name;self.git('-C',str(self.repo),'worktree','add','-qb','feature/'+name.replace(' ','-'),str(path));return path

class Worktrees(Repository,unittest.TestCase):
    def test_creation_identity_binds_guarded_local_and_explicit_self_routing(self):
        result=subprocess.run([str(HGS),'dirs','--creation',str(self.repo)],env=self.env,text=True,capture_output=True,timeout=10)
        self.assertEqual(result.returncode,0,result.stderr);identity=json.loads(result.stdout)['computer_id']
        self.assertEqual(str(uuid.UUID(identity)),identity)
        for prefix in ([],['@fixture']):
            guarded=subprocess.run([str(HGS),*prefix,'--expect-machine',identity,'--client','fixture-control','dirs',str(self.repo)],env=self.env,text=True,capture_output=True,timeout=10)
            self.assertEqual(guarded.returncode,0,guarded.stderr);self.assertEqual(json.loads(guarded.stdout)['path'],str(self.repo.resolve()))
        wrong=subprocess.run([str(HGS),'@fixture','--expect-machine',str(uuid.uuid4()),'dirs',str(self.repo)],env=self.env,text=True,capture_output=True,timeout=10)
        self.assertNotEqual(wrong.returncode,0);self.assertIn('identity changed',wrong.stderr)
        hop=subprocess.run([str(HGS),'--expect-machine',identity,'--client','fixture-control','@other.example.invalid','dirs',str(self.repo)],env=self.env,text=True,capture_output=True,timeout=10)
        self.assertNotEqual(hop.returncode,0);self.assertIn('requires a local command',hop.stderr)

        compact=subprocess.run([str(HGS),'--expect-machine',identity,'worktrees','--creation','--path',str(self.repo),'--refresh','--json'],env=self.env,text=True,capture_output=True,timeout=10)
        self.assertEqual(compact.returncode,0,compact.stderr);catalog=json.loads(compact.stdout)
        self.assertEqual(catalog['selected_branch'],'main');self.assertEqual(catalog['worktrees'],[])
        self.assertTrue(catalog['creation_receipts_v1'])
        compact_directory=json.loads(result.stdout)
        self.assertEqual(compact_directory['directories'],[])

    def receipt_create(self, request, branch='receipt-work', destination=None):
        return [str(HGS),'worktrees','create','--path',str(self.repo),'--common-dir',str(self.repo/'.git'),
                '--branch',branch,'--base','HEAD','--destination',str(destination or self.root/'receipt-checkout'),
                '--request-id',request,'--receipt','--json']

    def receipt_control(self, action, request):
        result=subprocess.run([str(HGS),'worktrees',action,'--request-id',request,'--json'],env=self.env,text=True,capture_output=True,timeout=5)
        self.assertEqual(result.returncode,0,result.stderr)
        return json.loads(result.stdout)

    def test_explicit_stop_seals_future_starts_without_inventing_a_past_result(self):
        state=Path(self.env['HGS_STATE_DIR']);store=state/'worktree-requests'
        store.mkdir(parents=True,mode=0o700);state.chmod(0o700)
        request=str(uuid.uuid4());args=self.receipt_create(request)
        original=[item for item in args[3:] if item!='--receipt']
        fingerprint=hashlib.sha256(json.dumps(original,ensure_ascii=False,separators=(',',':')).encode()).hexdigest()
        before=set(store.iterdir())
        self.assertEqual(self.receipt_control('status',request)['status'],'not_found')
        self.assertEqual(set(store.iterdir()),before)
        self.assertFalse((state/'worktree-requests.identity.json').exists(),'status must not initialize coverage')
        cancel=[str(HGS),'worktrees','cancel','--request-id',request,'--fingerprint',fingerprint,'--json']
        result=subprocess.run(cancel,env=self.env,text=True,capture_output=True,timeout=5)
        self.assertEqual(result.returncode,0,result.stderr);fence=json.loads(result.stdout)
        self.assertEqual(fence['status'],'fenced');self.assertEqual(fence['past_outcome'],'unknown')
        self.assertTrue(fence['future_starts_fenced']);self.assertNotIn('effects_started',fence)
        receipt=store/f'{request}.json';saved=receipt.read_bytes()
        for delayed in (args,[item for item in args if item!='--receipt']):
            result=subprocess.run(delayed,env=self.env,text=True,capture_output=True,timeout=8)
            self.assertEqual(result.returncode,0,result.stderr);self.assertEqual(json.loads(result.stdout),fence)
            self.assertFalse((self.root/'receipt-checkout').exists())
            self.assertEqual(self.git('-C',str(self.repo),'branch','--list','receipt-work'),'')
        self.assertEqual(self.receipt_control('status',request),fence)
        result=subprocess.run(cancel,env=self.env,text=True,capture_output=True,timeout=5)
        self.assertEqual(result.returncode,0,result.stderr);self.assertEqual(json.loads(result.stdout),fence)
        self.assertEqual(receipt.read_bytes(),saved)
        wrong=cancel.copy();wrong[wrong.index('--fingerprint')+1]='0'*64
        result=subprocess.run(wrong,env=self.env,text=True,capture_output=True,timeout=5)
        self.assertNotEqual(result.returncode,0);self.assertEqual(receipt.read_bytes(),saved)

    def test_a_replaced_empty_receipt_store_never_erases_the_original_fence(self):
        state=Path(self.env['HGS_STATE_DIR']);store=state/'worktree-requests'
        store.mkdir(parents=True,mode=0o700);state.chmod(0o700)
        request=str(uuid.uuid4());args=self.receipt_create(request)
        fingerprint=hashlib.sha256(json.dumps([item for item in args[3:] if item!='--receipt'],ensure_ascii=False,separators=(',',':')).encode()).hexdigest()
        cancel=[str(HGS),'worktrees','cancel','--request-id',request,'--fingerprint',fingerprint,'--json']
        result=subprocess.run(cancel,env=self.env,text=True,capture_output=True,timeout=5)
        self.assertEqual(result.returncode,0,result.stderr)
        saved=(store/f'{request}.json').read_bytes();retained=state/'original-requests-retained';store.rename(retained);store.mkdir(mode=0o700)
        for command in (cancel,args,[str(HGS),'worktrees','status','--request-id',request,'--json']):
            result=subprocess.run(command,env=self.env,text=True,capture_output=True,timeout=8)
            self.assertNotEqual(result.returncode,0)
        self.assertEqual((retained/f'{request}.json').read_bytes(),saved)
        self.assertFalse((store/f'{request}.json').exists());self.assertFalse((self.root/'receipt-checkout').exists())

    def test_receipt_replays_original_result_and_refuses_changed_arguments(self):
        request=str(uuid.uuid4());args=self.receipt_create(request)
        first=subprocess.run(args,env=self.env,text=True,capture_output=True,timeout=10)
        self.assertEqual(first.returncode,0,first.stderr);value=json.loads(first.stdout);self.assertEqual(value['status'],'created')
        (self.root/'receipt-checkout'/'keep.txt').write_text('local data')
        repeated=subprocess.run(args,env=self.env,text=True,capture_output=True,timeout=5)
        self.assertEqual(json.loads(repeated.stdout),value)
        self.assertEqual(self.receipt_control('status',request),value)
        changed=subprocess.run(self.receipt_create(request,branch='another',destination=self.root/'another'),env=self.env,text=True,capture_output=True,timeout=5)
        self.assertNotEqual(changed.returncode,0);self.assertFalse((self.root/'another').exists())
        self.assertEqual((self.root/'receipt-checkout'/'keep.txt').read_text(),'local data')

    def test_interrupted_or_corrupt_receipt_never_restarts_creation(self):
        request=str(uuid.uuid4());args=self.receipt_create(request)
        result=subprocess.run(args,env=self.env,text=True,capture_output=True,timeout=10)
        self.assertEqual(result.returncode,0,result.stderr)
        receipt=self.root/'state'/'worktree-requests'/f'{request}.json'
        saved=json.loads(receipt.read_text());saved['status']='running';receipt.write_text(json.dumps(saved))
        self.assertEqual(self.receipt_control('status',request)['status'],'unconfirmed')
        repeated=subprocess.run(args,env=self.env,text=True,capture_output=True,timeout=5)
        self.assertEqual(repeated.returncode,0,repeated.stderr);self.assertEqual(json.loads(repeated.stdout)['status'],'unconfirmed')
        receipt.write_text('{broken')
        repeated=subprocess.run(args,env=self.env,text=True,capture_output=True,timeout=5)
        self.assertNotEqual(repeated.returncode,0);self.assertEqual(receipt.read_text(),'{broken')

    def test_created_receipt_checks_current_ownership_and_keeps_replaced_marker(self):
        request=str(uuid.uuid4());result=subprocess.run(self.receipt_create(request),env=self.env,text=True,capture_output=True,timeout=10)
        self.assertEqual(result.returncode,0,result.stderr);value=json.loads(result.stdout)
        marker=Path(value['creation_owner']['metadata'])/'zerus-owner.json'
        marker.write_text('{replaced')
        status=self.receipt_control('status',request)
        self.assertEqual(status['status'],'created');self.assertIn('ownership changed',status['resource_error'])
        self.assertEqual(marker.read_text(),'{replaced');self.assertTrue((self.root/'receipt-checkout').is_dir())

    def test_definite_precheckout_refusal_is_recorded_without_effects(self):
        request=str(uuid.uuid4());args=self.receipt_create(request,branch='main')
        result=subprocess.run(args,env=self.env,text=True,capture_output=True,timeout=10)
        self.assertEqual(result.returncode,0,result.stderr);value=json.loads(result.stdout)
        self.assertEqual(value['status'],'failed');self.assertIs(value['effects_started'],False)
        self.assertFalse((self.root/'receipt-checkout').exists());self.assertEqual(self.receipt_control('status',request),value)

    @unittest.skipUnless(os.name=='posix','Unix hook process group qualification')
    def test_receipt_cancellation_stops_only_original_checkout_and_keeps_files(self):
        hook=self.repo/'.git'/'hooks'/'post-checkout';ready=self.root/'hook-ready'
        hook.write_text('#!'+sys.executable+'\nimport pathlib,time\npathlib.Path('+repr(str(ready))+').touch()\ntime.sleep(30)\n');hook.chmod(0o755)
        request=str(uuid.uuid4());child=subprocess.Popen(self.receipt_create(request),env=self.env,text=True,stdout=subprocess.PIPE,stderr=subprocess.PIPE)
        self.addCleanup(lambda: child.kill() if child.poll() is None else None)
        end=time.monotonic()+5
        while not ready.exists() and time.monotonic()<end:time.sleep(.01)
        self.assertTrue(ready.exists(),'Original checkout hook did not start')
        self.assertEqual(self.receipt_control('status',request)['status'],'running')
        duplicate=subprocess.run(self.receipt_create(request),env=self.env,text=True,capture_output=True,timeout=5)
        self.assertEqual(duplicate.returncode,0,duplicate.stderr);self.assertEqual(json.loads(duplicate.stdout)['status'],'running')
        self.assertEqual(self.receipt_control('cancel',request)['status'],'running')
        out,err=child.communicate(timeout=5);self.assertEqual(child.returncode,0,err)
        self.assertEqual(json.loads(out)['status'],'failed');self.assertEqual(self.receipt_control('status',request)['status'],'failed')
        self.assertTrue((self.root/'receipt-checkout').exists())
        self.assertEqual(self.git('-C',str(self.repo),'branch','--show-current'),'main')

    @unittest.skipUnless(os.name=='posix','Unix wrapper lifeline qualification')
    def test_receipt_worker_finishes_checkout_shutdown_after_wrapper_is_killed(self):
        hook=self.repo/'.git'/'hooks'/'post-checkout';ready=self.root/'hook-ready'
        hook.write_text('#!'+sys.executable+'\nimport pathlib,time\npathlib.Path('+repr(str(ready))+').touch()\ntime.sleep(30)\n');hook.chmod(0o755)
        request=str(uuid.uuid4());child=subprocess.Popen(self.receipt_create(request),env=self.env,text=True,stdout=subprocess.PIPE,stderr=subprocess.PIPE)
        self.addCleanup(lambda: child.kill() if child.poll() is None else None)
        end=time.monotonic()+5
        while not ready.exists() and time.monotonic()<end:time.sleep(.01)
        self.assertTrue(ready.exists(),'Original checkout hook did not start')
        child.kill();child.communicate(timeout=5)
        end=time.monotonic()+5;status=self.receipt_control('status',request)
        # A status reader can read the old running receipt just before the worker
        # writes its terminal receipt and releases its lock. That one snapshot
        # is conservatively unconfirmed; wait for the persisted terminal result.
        while status['status'] in ('running','unconfirmed') and time.monotonic()<end:
            time.sleep(.01);status=self.receipt_control('status',request)
        self.assertEqual(status['status'],'failed',status)
        self.assertTrue((self.root/'receipt-checkout').exists())

    def test_main_linked_subdirectory_symlink_and_identity(self):
        linked=self.linked();nested=linked/'nested';nested.mkdir();alias=self.root/'alias';alias.symlink_to(nested,target_is_directory=True)
        data=self.catalog(alias);self.assertEqual(data['state'],'ok');self.assertEqual(data['path'],str(nested.resolve()));self.assertEqual(data['selected_root'],str(linked.resolve()))
        self.assertEqual(data['common_dir'],str((self.repo/'.git').resolve()));self.assertEqual(data['machine'],'fixture')
        self.assertEqual([r['kind'] for r in data['worktrees']],['main','linked']);self.assertTrue(all(r['available'] for r in data['worktrees']))
        self.assertEqual(data['worktrees'][1]['branch'],'feature/linked-tree')
        self.assertEqual(self.catalog()['sampled_at'],data['sampled_at'])
    def test_plain_folder_unborn_bare_and_missing(self):
        plain=self.root/'plain';plain.mkdir();self.assertEqual(self.catalog(plain)['state'],'not_repo')
        self.assertFalse((plain/'.git').exists());self.assertEqual(self.catalog(self.root/'missing')['state'],'folder_unavailable')
        unborn=self.root/'unborn';self.git('init','-q',str(unborn));self.assertEqual(self.catalog(unborn)['state'],'ok')
        bare=self.root/'bare';self.git('clone','--bare','-q',str(self.repo),str(bare));data=self.catalog(bare)
        self.assertEqual(data['worktrees'][0]['kind'],'bare');self.assertIsNone(data['selected_root'])
    def test_locked_detached_missing_and_no_cleanup(self):
        linked=self.linked();self.git('-C',str(linked),'checkout','--detach','-q');self.git('-C',str(self.repo),'worktree','lock','--reason','keep this copy',str(linked))
        row=self.catalog()['worktrees'][1];self.assertTrue(row['detached']);self.assertTrue(row['locked']);self.assertEqual(row['locked_reason'],'keep this copy')
        self.git('-C',str(self.repo),'worktree','unlock',str(linked));shutil.rmtree(linked)
        row=self.catalog(refresh=True)['worktrees'][1];self.assertFalse(row['available']);self.assertTrue(row['prunable'])
        self.assertTrue((self.repo/'.git/worktrees/linked-tree').exists())
    def test_refresh_cache_and_independent_clones(self):
        before=self.catalog();self.linked();self.assertEqual(len(self.catalog()['worktrees']),1)
        self.assertEqual(len(self.catalog(refresh=True)['worktrees']),2)
        clone=self.root/'other'/'проект repo';clone.parent.mkdir();self.git('clone','-q',str(self.repo),str(clone))
        self.assertNotEqual(self.catalog(clone)['common_dir'],before['common_dir'])
    def test_submodule_identity_is_distinct_from_parent(self):
        source=self.root/'submodule-source';self.git('clone','-q',str(self.repo),str(source))
        self.git('-C',str(self.repo),'-c','protocol.file.allow=always','submodule','add','-q',str(source),'child')
        data=self.catalog(self.repo/'child');self.assertEqual(data['state'],'ok')
        self.assertEqual(data['worktrees'][0]['path'],str((self.repo/'child').resolve()))
        self.assertNotEqual(data['common_dir'],self.catalog()['common_dir'])
    def test_environment_cannot_redirect_discovery(self):
        self.env.update(GIT_DIR='/nonexistent',GIT_WORK_TREE='/nonexistent',GIT_COMMON_DIR='/nonexistent',GIT_CONFIG_COUNT='1',GIT_CONFIG_KEY_0='core.bare',GIT_CONFIG_VALUE_0='true')
        data=self.catalog();self.assertEqual(data['state'],'ok');self.assertEqual(data['worktrees'][0]['path'],str(self.repo.resolve()))
    def test_errors_are_distinct_and_keep_last_snapshot(self):
        before=self.catalog();self.fake("sys.stderr.write('fatal: permission denied\\n');sys.exit(1)\n")
        failed=self.catalog(refresh=True);self.assertEqual(failed['state'],'metadata_error');self.assertTrue(failed['stale']);self.assertEqual(failed['worktrees'],before['worktrees'])
        if not Path('/opt/homebrew/bin/git').exists() and not Path('/usr/local/bin/git').exists():
            (self.home/'.local/bin/git').unlink();self.env['PATH']='/nonexistent';self.assertEqual(self.catalog()['state'],'git_unavailable')
    def test_timeout_and_large_output_are_bounded(self):
        self.fake('time.sleep(20)\n');start=time.monotonic();self.assertEqual(self.catalog()['state'],'timeout');self.assertLess(time.monotonic()-start,5)
        self.fake("sys.stdout.write('x'*(2*1024*1024));sys.stdout.flush()\n");self.assertEqual(self.catalog()['state'],'too_large')
    def test_large_catalog_drains_pipes_and_marks_partial(self):
        common=str(self.repo/'.git')
        self.fake("if 'worktree' in sys.argv:\n for i in range(520):\n  sys.stdout.write('worktree /fixture/'+str(i)+'-'+('x'*150)+'\\0HEAD 0000000000000000000000000000000000000000\\0branch refs/heads/a\\0\\0')\nelif '--show-toplevel' in sys.argv: print("+repr(str(self.repo))+")\nelse: print("+repr(common)+")\n")
        data=self.catalog();self.assertEqual(data['state'],'ok');self.assertTrue(data['partial']);self.assertEqual(len(data['worktrees']),512)
        self.assertEqual(data['path'],str(self.repo.resolve()));self.assertEqual(data['selected_root'],str(self.repo.resolve()))
        self.assertNotIn(data['selected_root'],[row['path'] for row in data['worktrees']])
    def test_concurrent_catalog_requests_share_snapshot(self):
        linked=self.linked()
        with concurrent.futures.ThreadPoolExecutor() as pool:data=list(pool.map(self.catalog,[self.repo,linked]*3))
        self.assertTrue(all(v['state']=='ok' for v in data));self.assertEqual(len({v['sampled_at'] for v in data}),1)
    def create(self,destination=None,branch='feature/new',base='HEAD',extra=(),source=None):
        return subprocess.run([str(HGS),'worktrees','create','--path',str(source or self.repo),'--destination',str(destination or self.root/'new worktree'),
            '--branch',branch,'--base',base,'--request-id','test-create','--json',*extra],env=self.env,text=True,capture_output=True,timeout=10)
    def test_create_pins_base_preserves_source_and_refreshes_catalog(self):
        base=self.git('-C',str(self.repo),'rev-parse','HEAD')
        self.git('-C',str(self.repo),'-c','user.name=Fixture','-c','user.email=fixture@example.test','commit','--allow-empty','-qm','second')
        dirty=self.repo/'untracked.txt';dirty.write_text('keep my edits')
        self.catalog();destination=self.root/'новая копия'
        result=self.create(destination,base=base,extra=('--common-dir',str(self.repo/'.git')))
        self.assertEqual(result.returncode,0,result.stderr);data=json.loads(result.stdout)
        self.assertEqual(data['status'],'created');self.assertEqual(data['path'],str(destination.resolve()));self.assertEqual(data['request_id'],'test-create')
        self.assertEqual(self.git('-C',str(destination),'rev-parse','HEAD'),base)
        self.assertEqual(self.git('-C',str(destination),'branch','--show-current'),'feature/new')
        self.assertEqual(self.git('-C',str(self.repo),'branch','--show-current'),'main');self.assertEqual(dirty.read_text(),'keep my edits')
        self.assertEqual(len(self.catalog()['worktrees']),2)
    def test_create_inherits_only_a_safe_remote_upstream(self):
        remote=self.root/'remote.git';self.git('init','-q','--bare',str(remote))
        self.git('-C',str(self.repo),'remote','add','origin',str(remote));self.git('-C',str(self.repo),'push','-qu','origin','main')
        upstream=lambda path:subprocess.run([GIT,'-C',str(path),'rev-parse','--abbrev-ref','--symbolic-full-name','@{upstream}'],
            env=self.env,text=True,capture_output=True).stdout.strip()
        cases=[('head','HEAD',(),'origin/main'),('named','main',(),'origin/main'),
            ('commit',self.git('-C',str(self.repo),'rev-parse','HEAD'),(),''),
            ('pushes-upstream','HEAD',(('push.default','upstream'),),''),('local','HEAD',(('branch.main.remote','.'),),'')]
        for name,base,config,expected in cases:
            with self.subTest(name):
                for key,value in config:self.git('-C',str(self.repo),'config',key,value)
                try:
                    destination=self.root/name;result=self.create(destination,branch='feature/'+name,base=base)
                    self.assertEqual(result.returncode,0,result.stderr)
                    self.assertEqual(json.loads(result.stdout)['upstream'],expected or None)
                    self.assertEqual(upstream(destination),expected)
                    self.assertEqual(upstream(self.repo),'origin/main' if name!='local' else 'main')
                finally:
                    for key,_ in config:self.git('-C',str(self.repo),'config','--unset',key)
                    if name=='local':self.git('-C',str(self.repo),'config','branch.main.remote','origin')
        self.assertEqual(self.git('-C',str(remote),'for-each-ref','--format=%(refname)'),'refs/heads/main')
    def test_create_rejects_collisions_symlinks_and_repository_change(self):
        destination=self.root/'new worktree'
        existing=self.create(branch='main');self.assertNotEqual(existing.returncode,0);self.assertFalse(destination.exists())
        for branch in ('main/child','HEAD'):
            self.assertNotEqual(self.create(branch=branch).returncode,0);self.assertFalse(destination.exists())
        self.git('-C',str(self.repo),'branch','topic/child');self.assertNotEqual(self.create(branch='topic').returncode,0);self.assertFalse(destination.exists())
        destination.mkdir();keep=destination/'keep';keep.write_text('unchanged')
        self.assertNotEqual(self.create().returncode,0);self.assertEqual(keep.read_text(),'unchanged')
        alias=self.root/'alias';alias.symlink_to(self.root/'missing')
        self.assertNotEqual(self.create(alias).returncode,0);self.assertTrue(alias.is_symlink())
        changed=self.create(self.root/'different',extra=('--common-dir',str(self.root)))
        self.assertNotEqual(changed.returncode,0);self.assertIn('Repository changed',changed.stderr);self.assertFalse((self.root/'different').exists())
    def test_create_validates_inputs_and_dry_run_before_any_mutation(self):
        for branch,base in [('-bad','HEAD'),('bad..branch','HEAD'),('@{-1}','HEAD'),('good','does-not-exist'),('good','--help')]:
            with self.subTest(branch=branch,base=base):
                result=self.create(branch=branch,base=base);self.assertNotEqual(result.returncode,0,result.stdout);self.assertFalse((self.root/'new worktree').exists())
        result=self.create(extra=('--dry-run',));self.assertNotEqual(result.returncode,0);self.assertFalse((self.root/'new worktree').exists())
        self.assertNotEqual(self.create(self.repo/'.git'/'inside').returncode,0)
        self.assertNotEqual(self.create(self.root/'missing'/'child').returncode,0)
        plain=self.root/'plain';plain.mkdir();self.assertNotEqual(self.create(source=plain).returncode,0);self.assertFalse((plain/'.git').exists())
        self.assertEqual(self.git('-C',str(self.repo),'for-each-ref','--format=%(refname)','refs/heads'),'refs/heads/main')
    def test_create_from_linked_and_bare_repositories(self):
        linked=self.linked();self.assertEqual(self.create(source=linked).returncode,0)
        bare=self.root/'bare';self.git('clone','--bare','-q',str(self.repo),str(bare))
        result=self.create(self.root/'bare-checkout',branch='from-bare',source=bare);self.assertEqual(result.returncode,0,result.stderr)
        self.assertTrue((self.root/'bare-checkout'/'.git').is_file())
    def test_create_failure_keeps_checkout_and_branch(self):
        hook=self.repo/'.git/hooks/post-checkout';hook.write_text('#!/bin/sh\nprintf "hook-created" > keep.txt\nexit 1\n');hook.chmod(0o755)
        result=self.create();self.assertNotEqual(result.returncode,0);self.assertIn('have been kept',result.stderr)
        self.assertEqual((self.root/'new worktree/keep.txt').read_text(),'hook-created')
        self.assertEqual(self.git('-C',str(self.root/'new worktree'),'branch','--show-current'),'feature/new')
        retry=self.create();self.assertNotEqual(retry.returncode,0);self.assertIn('already exists',retry.stderr)
    def test_concurrent_create_reserves_destination(self):
        with concurrent.futures.ThreadPoolExecutor() as pool:results=list(pool.map(lambda branch:self.create(branch=branch),['first','second']))
        self.assertEqual(sum(r.returncode==0 for r in results),1)
        self.assertEqual(len(self.catalog()['worktrees']),2)

class Cleanup(Repository,unittest.TestCase):
    """Guarded removal, with an explicit content-bound exception for manual discard."""
    def setUp(self):
        super().setUp();(self.repo/'.gitignore').write_text('target/\nartifacts/\n');self.commit(self.repo,'ignore build output',add=True)
    def commit(self,path,message,add=False):
        if add:self.git('-C',str(path),'add','-A')
        self.git('-C',str(path),'-c','user.name=Fixture','-c','user.email=fixture@example.test','commit','--allow-empty','-qm',message)
    def age(self,path,days=2):
        gitdir=Path(self.git('-C',str(path),'rev-parse','--absolute-git-dir'));old=time.time()-days*86400
        for name in ('HEAD','index','logs/HEAD'):
            if (gitdir/name).exists():os.utime(gitdir/name,(old,old))
    def review(self,path=None,worktree=None):
        result=subprocess.run([str(HGS),'worktrees','review','--path',str(path or self.repo),*(['--worktree',str(worktree)] if worktree else []),'--json'],env=self.env,text=True,capture_output=True,timeout=30)
        self.assertEqual(result.returncode,0,result.stderr);return json.loads(result.stdout)
    def row(self,path,data=None):
        data=data or self.review();names={str(path),str(Path(path).resolve())}
        return next(row for row in data['worktrees'] if row['path'] in names)
    def remove(self,path,fingerprint,common=None,extra=()):
        return subprocess.run([str(HGS),'worktrees','remove','--path',str(path),'--common-dir',str(common or self.repo/'.git'),'--fingerprint',fingerprint,
            '--request-id','test-remove','--json',*extra],env=self.env,text=True,capture_output=True,timeout=30)
    def binding(self,name,cwd,archived=False,backend=None):
        state=self.root/'state'
        if archived:
            identity=str(uuid.uuid4());path=state/'archives'/(identity+'.json')
            record={'version':1,'name':name,'agent':'codex','archive_id':identity,'cwd':str(cwd),'archived_at':time.time()}
        elif backend=='dsh':
            path=state/'dsh/bindings'/(hashlib.sha256(name.encode()).hexdigest()+'.json');record={'backend':'dsh','name':name,'agent':'dsh','conversation_id':'session-'+str(uuid.uuid4()),'cwd':str(cwd)}
        else:
            path=state/(hashlib.sha256(name.encode()).hexdigest()+'.json');record={'version':1,'name':name,'agent':'codex','launch_dir':str(cwd),'cwd':str(self.root)}
        path.parent.mkdir(parents=True,exist_ok=True);path.write_text(json.dumps(record))
    def test_review_separates_ready_review_and_blocked_checkouts(self):
        merged=self.linked('merged');feature=self.linked('feature');(feature/'feature.txt').write_text('remaining work');self.commit(feature,'feature work',add=True)
        dirty=self.linked('dirty');(dirty/'.gitignore').write_text('changed\n')
        untracked=self.linked('untracked');(untracked/'notes.txt').write_text('keep')
        fresh=self.linked('fresh')
        for path in (merged,feature,dirty,untracked):self.age(path)
        data=self.review();self.assertEqual(data['state'],'ok');self.assertEqual(data['common_dir'],str((self.repo/'.git').resolve()));self.assertIn('main',data['base'])
        self.assertEqual(data['machine'],'fixture');self.assertFalse(data['partial'])
        main=self.row(self.repo,data);self.assertEqual(main['verdict'],'blocked');self.assertEqual(codes(main['reasons']),['main_checkout'])
        row=self.row(merged,data);self.assertEqual(row['verdict'],'ready',row);self.assertTrue(row['merged']);self.assertEqual(row['reasons'],[]);self.assertEqual(row['notes'],[])
        self.assertEqual(len(row['fingerprint']),64);self.assertEqual(row['branch'],'feature/merged');self.assertLess(row['last_activity'],time.time()-86400)
        row=self.row(feature,data);self.assertEqual(row['verdict'],'review');self.assertFalse(row['merged']);self.assertEqual(row['ahead'],1);self.assertEqual(codes(row['notes']),['not_merged'])
        row=self.row(dirty,data);self.assertEqual(row['verdict'],'blocked');self.assertEqual(codes(row['reasons']),['changes']);self.assertEqual(row['changes'],1)
        row=self.row(untracked,data);self.assertEqual(codes(row['reasons']),['untracked']);self.assertEqual(row['untracked'],1)
        row=self.row(fresh,data);self.assertEqual(row['verdict'],'review');self.assertEqual(codes(row['notes']),['recent'])
        one=self.review(merged/'.',worktree=merged);self.assertEqual([r['path'] for r in one['worktrees']],[str(merged.resolve())]);self.assertEqual(one['worktrees'][0]['fingerprint'],self.row(merged,data)['fingerprint'])
        plain=self.root/'plain';plain.mkdir();self.assertEqual(self.review(plain)['state'],'not_repo');self.assertFalse((plain/'.git').exists())
    def test_review_blocks_locks_nesting_processes_sessions_and_lost_commits(self):
        locked=self.linked('locked');self.git('-C',str(self.repo),'worktree','lock','--reason','in use',str(locked))
        parent=self.linked('parent');(parent/'artifacts').mkdir();nested=parent/'artifacts'/'nested';self.git('-C',str(self.repo),'worktree','add','-qb','nested',str(nested))
        busy=self.linked('busy');process=subprocess.Popen(['sleep','60'],cwd=busy/'.');self.addCleanup(process.wait);self.addCleanup(process.kill)
        bound=self.linked('bound');(bound/'src').mkdir();self.binding('codex/bound',bound/'src')
        deep=self.linked('deep');self.binding('dsh/deep',deep,backend='dsh')
        archived=self.linked('archived');self.binding('codex/old',archived,archived=True)
        lost=self.linked('lost');self.git('-C',str(lost),'checkout','-q','--detach');self.commit(lost,'only here')
        kept=self.linked('kept');self.git('-C',str(kept),'checkout','-q','--detach','main')
        for path in (locked,parent,nested,busy,bound,deep,archived,lost,kept):self.age(path)
        # Raw fixture files have no verified compact coverage yet. Review must
        # stay conservative until an ordinary archive read reconciles them.
        cold=self.row(archived,self.review())
        self.assertIn('archives_unavailable',codes(cold['notes']))
        page=subprocess.run([str(HGS),'archives','--json'],input='{"limit":1}',env=self.env,text=True,capture_output=True,timeout=20)
        self.assertEqual(page.returncode,0,page.stdout+page.stderr)
        data=self.review()
        row=self.row(locked,data);self.assertEqual(codes(row['reasons']),['locked']);self.assertIn('in use',row['reasons'][0]['message'])
        row=self.row(parent,data);self.assertEqual(codes(row['reasons']),['nested']);self.assertEqual(row['nested'],[str(nested.resolve())])
        row=self.row(busy,data);self.assertEqual(codes(row['reasons']),['processes']);self.assertIn(process.pid,[p['pid'] for p in row['processes']])
        row=self.row(bound,data);self.assertEqual(codes(row['reasons']),['sessions']);self.assertEqual(row['sessions'],['codex/bound'])
        self.assertEqual(self.row(deep,data)['sessions'],['dsh/deep'])
        row=self.row(archived,data);self.assertEqual(row['verdict'],'review');self.assertEqual(codes(row['notes']),['archived_sessions']);self.assertEqual(row['archived_sessions'],1)
        row=self.row(lost,data);self.assertEqual(codes(row['reasons']),['detached_unreachable'])
        row=self.row(kept,data);self.assertEqual(row['verdict'],'ready',row);self.assertTrue(row['detached'])
    @unittest.skipUnless(sys.platform in ('linux', 'darwin'), 'Native open-file membership')
    def test_an_open_file_blocks_removal_even_when_the_program_works_elsewhere(self):
        tree=self.linked('held file');self.age(tree)
        child=subprocess.Popen([sys.executable,'-I','-c',
            'import sys,time; held=open(sys.argv[1],"rb"); print("ready",flush=True); time.sleep(90)',
            str(tree/'.gitignore')],cwd=self.root,stdout=subprocess.PIPE,text=True)
        self.addCleanup(child.stdout.close);self.addCleanup(child.wait)
        self.addCleanup(lambda: child.poll() is None and child.kill())
        self.assertEqual(child.stdout.readline().strip(),'ready')
        row=self.row(tree)
        self.assertIn('processes',codes(row['reasons']),row)
        self.assertIn(child.pid,[p['pid'] for p in row['processes']])
        self.assertNotIn('process_fingerprint',row)
        self.assertTrue(row['process_coverage']['complete'])
        self.assertTrue(row['process_coverage']['open_files_checked'])
        refused=self.remove(tree,row['fingerprint'])
        self.assertNotEqual(refused.returncode,0)
        self.assertTrue(tree.exists());self.assertIsNone(child.poll())

    def test_review_reports_ignored_data_and_missing_checkouts(self):
        built=self.linked('built');(built/'target').mkdir();(built/'target'/'big.bin').write_bytes(b'x'*1000);(built/'target'/'small').write_bytes(b'y'*24)
        gone=self.linked('gone');shutil.rmtree(gone);self.age(built)
        data=self.review();row=self.row(built,data)
        self.assertEqual(row['verdict'],'review',row);self.assertIn('ignored_data',codes(row['notes']));self.assertEqual(row['ignored'],[{'path':'target','bytes':1024,'complete':True}]);self.assertEqual(row['ignored_bytes'],1024)
        row=self.row(gone,data);self.assertEqual(row['verdict'],'missing');self.assertFalse(row['available'])
    @unittest.skipUnless(sys.platform == 'linux', 'Linux protected-process review')
    def test_manual_process_confirmation_survives_churn_but_never_overrides_other_blockers(self):
        def protected():
            child=subprocess.Popen([sys.executable,'-I','-c',
                'import ctypes,time; assert ctypes.CDLL(None).prctl(4,0,0,0,0)==0; print("ready",flush=True); time.sleep(90)'],
                cwd=self.root,stdout=subprocess.PIPE,text=True)
            self.addCleanup(child.stdout.close);self.addCleanup(child.wait);self.addCleanup(lambda: child.poll() is None and child.kill())
            self.assertEqual(child.stdout.readline().strip(),'ready');return child
        tree=self.linked('manual');self.age(tree);unknown=protected();row=self.row(tree)
        self.assertEqual(codes(row['reasons']),['processes_unavailable'])
        # A host may hide other programs too (a user's systemd manager).
        own=next(p for p in row['unreadable_processes'] if p['pid']==unknown.pid)
        self.assertTrue(own['identity']);self.assertRegex(row['process_fingerprint'],r'^v2:[0-9]+:[a-f0-9]{64}$')
        refused=self.remove(tree,row['fingerprint']);self.assertNotEqual(refused.returncode,0);self.assertTrue(tree.exists())
        stale=self.remove(tree,row['fingerprint'],extra=('--process-fingerprint','0'*64))
        self.assertNotEqual(stale.returncode,0);self.assertIn('confirmation is stale',stale.stderr)
        automatic=self.remove(tree,row['fingerprint'],extra=('--automatic','--process-fingerprint',row['process_fingerprint']))
        self.assertNotEqual(automatic.returncode,0);self.assertTrue(tree.exists())
        churn=self.linked('churn');self.age(churn);old=self.row(churn)
        unknown.terminate();unknown.wait(timeout=5);protected();changed=self.row(tree)
        allowed=self.remove(churn,old['fingerprint'],extra=('--process-fingerprint',old['process_fingerprint']))
        self.assertEqual(allowed.returncode,0,allowed.stderr);self.assertFalse(churn.exists())
        known=subprocess.Popen(['sleep','90'],cwd=tree)
        self.addCleanup(lambda: known.poll() is None and (known.terminate(),known.wait(timeout=5)))
        busy=self.row(tree);self.assertIn('processes',codes(busy['reasons']));self.assertIn(known.pid,[p['pid'] for p in busy['processes']])
        self.assertNotIn('process_fingerprint',busy)
        refused=self.remove(tree,changed['fingerprint'],extra=('--process-fingerprint',changed['process_fingerprint']))
        self.assertNotEqual(refused.returncode,0);known.terminate();known.wait()
        self.binding('codex/keep',tree);bound=self.row(tree)
        self.assertIn('sessions',codes(bound['reasons']));self.assertNotIn('process_fingerprint',bound)
        refused=self.remove(tree,changed['fingerprint'],extra=('--process-fingerprint',changed['process_fingerprint']))
        self.assertNotEqual(refused.returncode,0)
        for file in (self.root/'state').glob('*.json'):file.unlink()
        (tree/'notes.txt').write_text('retain changes');dirty=self.row(tree)
        # Changes beside hidden processes also need the discard (zerus-k2ga).
        self.assertFalse(dirty['discardable']);self.assertTrue(dirty['discardable_with_process_consent'])
        for consent in (changed['process_fingerprint'],dirty['process_fingerprint']):
            refused=self.remove(tree,dirty['fingerprint'],extra=('--process-fingerprint',consent))
            self.assertNotEqual(refused.returncode,0);self.assertEqual((tree/'notes.txt').read_text(),'retain changes')
        (tree/'notes.txt').unlink()
        (tree/'target').mkdir();(tree/'target'/'local.txt').write_text('synthetic local data')
        current=self.row(tree)
        refused=self.remove(tree,current['fingerprint'],extra=('--process-fingerprint',current['process_fingerprint']))
        self.assertNotEqual(refused.returncode,0);self.assertIn('explicitly confirm',refused.stderr)
        allowed=self.remove(tree,current['fingerprint'],extra=('--process-fingerprint',current['process_fingerprint'],
            '--ignored-fingerprint',current['ignored_fingerprint']))
        self.assertEqual(allowed.returncode,0,allowed.stderr);self.assertFalse(tree.exists())
        self.assertIn('feature/manual',self.git('-C',str(self.repo),'branch','--list','feature/manual'))

    @unittest.skipUnless(sys.platform == 'linux', 'Linux protected-process review')
    def test_a_dirty_checkout_beside_hidden_processes_needs_both_explicit_consents(self):
        # zerus-k2ga: the discard and the bounded process acceptance together,
        # each refused alone, and never for automatic cleanup or another blocker.
        child=subprocess.Popen([sys.executable,'-I','-c',
            'import ctypes,time; assert ctypes.CDLL(None).prctl(4,0,0,0,0)==0; print("ready",flush=True); time.sleep(90)'],
            cwd=self.root,stdout=subprocess.PIPE,text=True)
        self.addCleanup(child.stdout.close);self.addCleanup(child.wait);self.addCleanup(lambda: child.poll() is None and child.kill())
        self.assertEqual(child.stdout.readline().strip(),'ready')
        tree=self.linked('dirty manual');(tree/'work.txt').write_text('committed');self.commit(tree,'unmerged work',add=True)
        (tree/'work.txt').write_text('modified');(tree/'notes.txt').write_text('local notes');self.age(tree)
        row=self.row(tree)
        self.assertEqual(sorted(codes(row['reasons'])),['changes','processes_unavailable','untracked'])
        self.assertFalse(row['discardable']);self.assertTrue(row['discardable_with_process_consent'])
        self.assertRegex(row['process_fingerprint'],r'^v2:[0-9]+:[a-f0-9]{64}$');self.assertTrue(row['discard_fingerprint'])
        both=('--discard-fingerprint',row['discard_fingerprint'],'--process-fingerprint',row['process_fingerprint'])
        def kept(result,text):
            self.assertNotEqual(result.returncode,0);self.assertIn(text,result.stderr)
            self.assertEqual((tree/'work.txt').read_text(),'modified');self.assertEqual((tree/'notes.txt').read_text(),'local notes')
        kept(self.remove(tree,row['fingerprint']),'Cannot remove')
        kept(self.remove(tree,row['fingerprint'],extra=both[:2]),'cannot check whether some running programs')
        kept(self.remove(tree,row['fingerprint'],extra=both[2:]),'Cannot remove')
        # Automatic cleanup never uses the process acceptance (Y1).
        kept(self.remove(tree,row['fingerprint'],extra=('--automatic',*both)),'Process confirmation is stale')
        # A known use or a saved session is still a blocker no consent overrides.
        known=subprocess.Popen(['sleep','90'],cwd=tree)
        self.addCleanup(lambda: known.poll() is None and (known.terminate(),known.wait(timeout=5)))
        busy=self.row(tree);self.assertIn('processes',codes(busy['reasons']))
        self.assertNotIn('process_fingerprint',busy);self.assertNotIn('discardable_with_process_consent',busy)
        kept(self.remove(tree,busy['fingerprint'],extra=both),'')
        known.terminate();known.wait(timeout=5)
        self.binding('codex/keep',tree);bound=self.row(tree)
        self.assertNotIn('process_fingerprint',bound);self.assertNotIn('discardable_with_process_consent',bound)
        kept(self.remove(tree,bound['fingerprint'],extra=('--discard-fingerprint',bound['discard_fingerprint'],'--process-fingerprint',row['process_fingerprint'])),'')
        for file in (self.root/'state').glob('*.json'):file.unlink()
        row=self.row(tree);result=self.remove(tree,row['fingerprint'],extra=('--discard-fingerprint',row['discard_fingerprint'],
            '--process-fingerprint',row['process_fingerprint'],'--delete-branch'))
        self.assertEqual(result.returncode,0,result.stderr);self.assertFalse(tree.exists())
        self.assertFalse(json.loads(result.stdout)['branch_deleted'])
        self.assertTrue(self.git('-C',str(self.repo),'show-ref','refs/heads/feature/dirty-manual'))

    @unittest.skipUnless(sys.platform == 'linux', 'Linux protected launch ancestors')
    def test_manual_process_confirmation_survives_separate_protected_launchers(self):
        tree=self.linked('remote manual');self.age(tree)
        wrapper='''import ctypes,json,os,subprocess,sys
assert ctypes.CDLL(None).prctl(4,0,0,0,0)==0
p=subprocess.run(sys.argv[1:],capture_output=True,text=True,timeout=25)
print(json.dumps(dict(pid=os.getpid(),rc=p.returncode,stdout=p.stdout,stderr=p.stderr)))
'''
        def invocation(args,cwd=None):
            p=subprocess.run([sys.executable,'-I','-c',wrapper,str(HGS),'worktrees',*args,'--json'],
                cwd=cwd or self.root,env=self.env,capture_output=True,text=True,timeout=30)
            self.assertEqual(p.returncode,0,p.stderr);return json.loads(p.stdout)
        def review():
            p=invocation(['review','--path',str(self.repo),'--worktree',str(tree)])
            self.assertEqual(p['rc'],0,p['stderr']);row=json.loads(p['stdout'])['worktrees'][0]
            self.assertEqual(codes(row['reasons']),['processes_unavailable'])
            own=next(r for r in row['unreadable_processes'] if r['pid']==p['pid'])
            self.assertTrue(own['identity'])
            return p['pid'],row
        first,row=review();second,current=review()
        self.assertNotEqual(first,second)
        self.assertEqual(row['fingerprint'],current['fingerprint'])
        self.assertTrue(current['process_fingerprint'].startswith('v2:'))
        self.assertTrue(next(r for r in current['unreadable_processes'] if r['pid']==second)['invocation_ancestor'])
        args=['remove','--path',str(tree),'--common-dir',str(self.repo/'.git'),
            '--fingerprint',row['fingerprint'],'--process-fingerprint',row['process_fingerprint']]
        automatic=invocation([*args,'--automatic'])
        self.assertNotEqual(automatic['rc'],0);self.assertTrue(tree.exists())
        # Readable use by an ancestor/this invocation remains a hard blocker.
        busy=invocation(args,cwd=tree)
        self.assertNotEqual(busy['rc'],0);self.assertTrue(tree.exists())
        result=invocation(args)
        self.assertEqual(result['rc'],0,result['stderr']);self.assertFalse(tree.exists())
        self.assertIn('feature/remote-manual',self.git('-C',str(self.repo),'branch','--list','feature/remote-manual'))

    def test_remove_rechecks_fingerprint_and_keeps_branch(self):
        tree=self.linked('task');self.age(tree);before=self.row(tree);self.catalog()
        self.commit(tree,'late work')
        stale=self.remove(tree,before['fingerprint']);self.assertNotEqual(stale.returncode,0);self.assertIn('changed since the review',stale.stderr);self.assertTrue(tree.exists())
        refusal=json.loads(stale.stdout);self.assertEqual(refusal['status'],'kept');self.assertEqual(refusal['operation'],'worktree_remove');self.assertEqual(refusal['request_id'],'test-remove')
        result=self.remove(tree,self.row(tree)['fingerprint']);self.assertEqual(result.returncode,0,result.stderr)
        data=json.loads(result.stdout);self.assertEqual(data['status'],'removed');self.assertEqual(data['request_id'],'test-remove');self.assertFalse(data['branch_deleted'])
        self.assertEqual(data['path'],str(tree.resolve()));self.assertEqual(data['common_dir'],str((self.repo/'.git').resolve()))
        self.assertFalse(tree.exists());self.assertIn('feature/task',self.git('-C',str(self.repo),'branch','--list','feature/task'))
        self.assertNotIn(str(tree.resolve()),[r['path'] for r in self.catalog()['worktrees']])
    def test_remove_failure_after_git_admission_has_an_unknown_structured_outcome(self):
        tree=self.linked('git-failure');self.age(tree);row=self.row(tree)
        self.fake("if any(sys.argv[i:i+2]==['worktree','remove'] for i in range(len(sys.argv)-1)):\n sys.stderr.write('fixture removal failure\\n');sys.exit(1)\nelse: os.execv("+repr(GIT)+",["+repr(GIT)+",*sys.argv[1:]])\n")
        result=self.remove(tree,row['fingerprint'])
        self.assertNotEqual(result.returncode,0);self.assertTrue(tree.exists())
        outcome=json.loads(result.stdout)
        self.assertEqual(outcome['status'],'unconfirmed');self.assertEqual(outcome['outcome_version'],1)
        self.assertEqual(outcome['request_id'],'test-remove');self.assertEqual(outcome['path'],str(tree))
        self.assertIn('fixture removal failure',outcome['detail'])

    def test_remove_deletes_ignored_data_and_only_merged_branches(self):
        merged=self.linked('merged');(merged/'target').mkdir();(merged/'target'/'out').write_text('build');self.age(merged)
        reviewed=self.row(merged);result=self.remove(merged,reviewed['fingerprint'],extra=('--delete-branch','--ignored-fingerprint',reviewed['ignored_fingerprint']));self.assertEqual(result.returncode,0,result.stderr)
        data=json.loads(result.stdout);self.assertTrue(data['branch_deleted']);self.assertFalse(merged.exists());self.assertEqual(self.git('-C',str(self.repo),'branch','--list','feature/merged'),'')
        feature=self.linked('feature');self.commit(feature,'unmerged');self.age(feature)
        result=self.remove(feature,self.row(feature)['fingerprint'],extra=('--delete-branch',));self.assertEqual(result.returncode,0,result.stderr)
        data=json.loads(result.stdout);self.assertFalse(data['branch_deleted']);self.assertIn('not merged',data['branch_error']);self.assertFalse(feature.exists())
        self.assertIn('feature/feature',self.git('-C',str(self.repo),'branch','--list','feature/feature'))
    def test_remove_refuses_blocked_checkouts_without_changes(self):
        dirty=self.linked('dirty');(dirty/'notes.txt').write_text('keep')
        result=self.remove(dirty,self.row(dirty)['fingerprint']);self.assertNotEqual(result.returncode,0);self.assertIn('untracked',result.stderr.lower());self.assertEqual((dirty/'notes.txt').read_text(),'keep')
        parent=self.linked('parent');(parent/'artifacts').mkdir();nested=parent/'artifacts'/'nested';self.git('-C',str(self.repo),'worktree','add','-qb','nested',str(nested));(nested/'work.txt').write_text('unsaved')
        result=self.remove(parent,self.row(parent)['fingerprint']);self.assertNotEqual(result.returncode,0);self.assertIn('nested',result.stderr.lower());self.assertEqual((nested/'work.txt').read_text(),'unsaved')
        main=self.row(self.repo);self.assertNotEqual(self.remove(self.repo,main['fingerprint'] or '0'*64).returncode,0);self.assertTrue((self.repo/'.git').is_dir())
        clean=self.linked('clean');row=self.row(clean)
        other=self.root/'other';self.git('init','-q',str(other))
        changed=self.remove(clean,row['fingerprint'],common=other/'.git');self.assertNotEqual(changed.returncode,0);self.assertIn('Repository changed',changed.stderr);self.assertTrue(clean.exists())
        self.assertNotEqual(self.remove(clean,row['fingerprint'],extra=('--dry-run',)).returncode,0);self.assertTrue(clean.exists())
        dry=subprocess.run([str(HGS),'--dry-run','worktrees','remove','--path',str(clean),'--common-dir',str(self.repo/'.git'),'--fingerprint',row['fingerprint']],env=self.env,text=True,capture_output=True,timeout=30)
        self.assertNotEqual(dry.returncode,0);self.assertTrue(clean.exists())
        relative=subprocess.run([str(HGS),'worktrees','remove','--path','clean','--common-dir',str(self.repo/'.git'),'--fingerprint',row['fingerprint'],'--json'],cwd=self.root,env=self.env,text=True,capture_output=True,timeout=30)
        self.assertNotEqual(relative.returncode,0);self.assertTrue(clean.exists())
    # Partial removal (zerus-k403): Git deregisters a worktree before it deletes
    # the files, so a readonly ignored folder can leave data Git no longer tracks.
    def partial(self,name):
        if os.geteuid()==0:self.skipTest('root ignores directory permissions')
        tree=self.linked(name);readonly=tree/'target'/'readonly';readonly.mkdir(parents=True);(readonly/'out').write_text('build')
        readonly.chmod(0o555);self.addCleanup(lambda:readonly.exists() and readonly.chmod(0o755))
        self.age(tree);row=self.row(tree)
        return tree,row,self.remove(tree,row['fingerprint'],extra=('--ignored-fingerprint',row['ignored_fingerprint']))
    def leftover(self,path,data=None):
        data=data or self.review();names={str(path),str(Path(path).resolve())}
        return next((row for row in data.get('leftovers',[]) if row['path'] in names),None)
    def keep(self,path,fingerprint):
        return subprocess.run([str(HGS),'worktrees','keep-folder','--path',str(path),'--common-dir',str(self.repo/'.git'),'--fingerprint',fingerprint,
            '--request-id','test-keep','--json'],env=self.env,text=True,capture_output=True,timeout=30)
    def records(self):
        import sqlite3
        database=self.root/'state'/'worktree-maintenance'/'queue.sqlite3'
        if not database.exists():return []
        with contextlib.closing(sqlite3.connect(database)) as db:return db.execute('SELECT path,status FROM removals ORDER BY path').fetchall()
    def test_partial_removal_keeps_a_reviewable_leftover_and_never_replays(self):
        tree,row,result=self.partial('partial')
        self.assertNotEqual(result.returncode,0);self.assertIn('files remain',result.stderr)
        self.assertEqual(json.loads(result.stdout)['status'],'unconfirmed')
        self.assertEqual((tree/'target'/'readonly'/'out').read_text(),'build')
        data=self.review();self.assertNotIn(str(tree.resolve()),[r['path'] for r in data['worktrees']])
        left=self.leftover(tree,data);self.assertIsNotNone(left,data)
        self.assertEqual((left['kind'],left['verdict']),('leftover','partially_removed'))
        self.assertIn('partially_removed',codes(left['notes']))
        self.assertEqual(left['removal']['status'],'partial');self.assertEqual(left['removal']['request_id'],'test-remove')
        self.assertEqual(left['removal']['fingerprint'],row['fingerprint']);self.assertIn('Permission denied',left['removal']['detail'])
        self.assertEqual(left['branch'],row['branch']);self.assertEqual(left['branch_head'],row['head']);self.assertTrue(left['branch_retained'])
        self.assertTrue(left['size_complete']);self.assertGreater(left['size'],0);self.assertEqual(left['sessions'],[])
        # Never replayed: the leftover is no longer one of the repository's worktrees.
        again=self.remove(tree,row['fingerprint'],extra=('--ignored-fingerprint',row['ignored_fingerprint']))
        self.assertNotEqual(again.returncode,0);self.assertEqual(json.loads(again.stdout)['status'],'kept')
        self.assertEqual((tree/'target'/'readonly'/'out').read_text(),'build')
        self.assertEqual(self.leftover(tree)['fingerprint'],left['fingerprint'])
        self.assertEqual(self.records(),[(str(tree.resolve()),'partial')])
    def test_keep_folder_forgets_only_the_record_after_a_fresh_bound_review(self):
        tree,row,_=self.partial('kept');left=self.leftover(tree)
        stale=self.keep(tree,'0'*64);self.assertNotEqual(stale.returncode,0);self.assertIn('changed since the review',stale.stderr)
        # A refusal before the record is touched is a request-bound outcome.
        refused=json.loads(stale.stdout)
        self.assertEqual((refused['operation'],refused['outcome_version'],refused['status'],refused['request_id'],refused['path']),
            ('worktree_leftover_keep',1,'kept','test-keep',str(tree)))
        self.assertIn('changed since the review',refused['detail']);self.assertEqual(len(self.records()),1)
        (tree/'later.txt').write_text('new local file')
        changed=self.keep(tree,left['fingerprint']);self.assertNotEqual(changed.returncode,0);self.assertIn('changed since the review',changed.stderr)
        left=self.leftover(tree);kept=self.keep(tree,left['fingerprint']);self.assertEqual(kept.returncode,0,kept.stderr)
        value=json.loads(kept.stdout);self.assertEqual((value['status'],value['request_id']),('kept','test-keep'))
        self.assertEqual((tree/'target'/'readonly'/'out').read_text(),'build');self.assertTrue((tree/'later.txt').exists())
        self.assertIsNone(self.leftover(tree));self.assertEqual(self.records(),[])
        self.assertIn(row['branch'],self.git('-C',str(self.repo),'branch','--list',row['branch']))
        again=self.keep(tree,left['fingerprint']);self.assertNotEqual(again.returncode,0);self.assertIn('No partial-removal record',again.stderr)
        absent=json.loads(again.stdout);self.assertEqual((absent['status'],absent['operation']),('kept','worktree_leftover_keep'))
        self.assertIn('No partial-removal record',absent['detail'])
    def test_keep_folder_reports_a_failure_after_forgetting_started_as_unconfirmed(self):
        import sqlite3
        tree,_,_=self.partial('forget-fails');left=self.leftover(tree)
        database=self.root/'state'/'worktree-maintenance'/'queue.sqlite3'
        # Reads still work; only forgetting the record fails, after every check passed.
        with contextlib.closing(sqlite3.connect(database)) as db,db:
            db.execute("CREATE TRIGGER synthetic_forget_failure BEFORE DELETE ON removals BEGIN SELECT RAISE(ABORT,'synthetic forget failure'); END")
        result=self.keep(tree,left['fingerprint']);self.assertNotEqual(result.returncode,0)
        outcome=json.loads(result.stdout)
        self.assertEqual((outcome['operation'],outcome['status'],outcome['request_id']),('worktree_leftover_keep','unconfirmed','test-keep'))
        self.assertIn('synthetic forget failure',outcome['detail'])
        self.assertEqual(self.records(),[(str(tree.resolve()),'partial')])
        self.assertEqual((tree/'target'/'readonly'/'out').read_text(),'build')
    def test_an_unreadable_record_store_is_never_taken_as_no_record(self):
        if os.geteuid()==0:self.skipTest('root ignores directory permissions')
        tree,_,_=self.partial('unreadable-store');left=self.leftover(tree)
        store=self.root/'state'/'worktree-maintenance';store.chmod(0o000)
        try:
            kept=self.keep(tree,left['fingerprint']);data=self.review()
            catalog=self.catalog()
        finally:
            store.chmod(0o700)
        self.assertNotEqual(kept.returncode,0);outcome=json.loads(kept.stdout)
        self.assertEqual(outcome['status'],'kept')
        self.assertNotIn('No partial-removal record',outcome['detail']);self.assertIn('Permission denied',outcome['detail'])
        self.assertEqual(data['leftovers'],[]);self.assertIn('Permission denied',data['leftovers_unavailable'])
        self.assertNotIn('leftovers',catalog)
        self.assertEqual(self.records(),[(str(tree.resolve()),'partial')])
    def hold_repository_lock(self):
        import fcntl,hashlib
        common=str((self.repo/'.git').resolve())
        path=self.root/'state'/'worktree-cache'/('repo-'+hashlib.sha256(common.encode()).hexdigest()+'.lock')
        self.assertTrue(path.parent.is_dir())
        handle=os.fdopen(os.open(path,os.O_RDWR|os.O_CREAT,0o600),'r+');fcntl.flock(handle,fcntl.LOCK_EX)
        return handle
    # Mutation D: a record whose worktree Git still lists is resolved only while
    # no other worktree operation holds the repository lock.
    def test_a_registered_record_is_forgotten_only_under_the_repository_lock(self):
        import sqlite3
        self.partial('lock-witness');live=self.linked('locked-live')
        database=self.root/'state'/'worktree-maintenance'/'queue.sqlite3';common=str((self.repo/'.git').resolve())
        with contextlib.closing(sqlite3.connect(database)) as db,db:
            db.execute("INSERT INTO removals VALUES (?,?,?,?,?,?,?,?,?)",(str(live.resolve()),common,'feature/locked-live','0'*40,'f'*64,'',1.0,'pending',''))
        held=self.hold_repository_lock()
        try:
            data=self.review()
            self.assertIsNone(self.leftover(live,data))
            self.assertIn((str(live.resolve()),'pending'),self.records())
        finally:
            held.close()
        self.review();self.assertNotIn(str(live.resolve()),[path for path,_ in self.records()])
    # Mutation E: keep-folder waits for the repository lock and then refuses.
    def test_keep_folder_never_runs_while_another_worktree_operation_holds_the_lock(self):
        tree,_,_=self.partial('lock-keep');left=self.leftover(tree)
        held=self.hold_repository_lock()
        try:
            result=self.keep(tree,left['fingerprint'])
        finally:
            held.close()
        self.assertNotEqual(result.returncode,0);self.assertIn('Another worktree operation is running',result.stderr)
        self.assertEqual(json.loads(result.stdout)['status'],'kept')
        self.assertEqual(self.records(),[(str(tree.resolve()),'partial')])
        kept=self.keep(tree,left['fingerprint']);self.assertEqual(kept.returncode,0,kept.stderr);self.assertEqual(self.records(),[])
    # Mutation I: a dangling link at the leftover path is something there, not
    # a vanished folder; only a missing name resolves the record.
    def test_a_dangling_link_at_a_leftover_path_is_not_taken_as_gone(self):
        import sqlite3
        self.partial('link-witness');database=self.root/'state'/'worktree-maintenance'/'queue.sqlite3'
        link=self.root/'dangling-leftover';os.symlink(self.root/'nowhere',link);missing=self.root/'missing-leftover'
        with contextlib.closing(sqlite3.connect(database)) as db,db:
            db.executemany("INSERT INTO removals VALUES (?,?,?,?,?,?,?,?,?)",[(str(p),str(self.root/'gone-repository/.git'),'b','0'*40,'f'*64,'',1.0,'partial','') for p in (link,missing)])
        listed=subprocess.run([str(HGS),'worktrees','leftovers','--json'],env=self.env,text=True,capture_output=True,timeout=60)
        self.assertEqual(listed.returncode,0,listed.stderr)
        paths=[path for path,_ in self.records()]
        self.assertIn(str(link),paths);self.assertNotIn(str(missing),paths)
        self.assertIn(str(link),[row['path'] for row in json.loads(listed.stdout)['leftovers']])
    # zerus-ty94 #2: a targeted review reports that worktree's own leftover
    # and nothing about any other record.
    def test_a_targeted_review_reports_only_its_own_leftover(self):
        tree,_,_=self.partial('targeted');other,_,_=self.partial('untargeted')
        data=self.review(worktree=tree)
        self.assertEqual(data['worktrees'],[])
        self.assertEqual([row['path'] for row in data['leftovers']],[str(tree.resolve())])
        self.assertEqual(data['leftovers'][0]['verdict'],'partially_removed');self.assertFalse(data['leftovers_partial'])
        live=self.linked('still-live');data=self.review(worktree=live)
        self.assertEqual([row['path'] for row in data['worktrees']],[str(live.resolve())]);self.assertEqual(data['leftovers'],[])
        self.assertEqual(len(self.records()),2)
    def volumes(self):
        import sqlite3
        database=self.root/'state'/'worktree-maintenance'/'queue.sqlite3'
        with contextlib.closing(sqlite3.connect(database)) as db:return db.execute('SELECT path,folder,repository FROM removal_volumes ORDER BY path').fetchall()
    # zerus-ty94 #5: a reservation records which filesystem held the folder
    # and the repository; a missing path counts as gone only there.
    @unittest.skipUnless(sys.platform in ('linux','darwin'),'volume identities exist on Linux and macOS')
    def test_a_vanished_record_resolves_only_on_its_recorded_filesystem(self):
        import sqlite3
        tree,_,_=self.partial('volume-witness')
        [(path,folder,repository)]=self.volumes()
        self.assertEqual(path,str(tree.resolve()))
        if not folder:self.skipTest('this filesystem has no identity (zero fsid)')
        self.assertTrue(folder.startswith(('linux:','macos:')));self.assertEqual(folder,repository)
        database=self.root/'state'/'worktree-maintenance'/'queue.sqlite3';gone_repository=str(self.root/'gone-repository/.git')
        cases={'older-cli':None,'unrecorded':('',''),'same':(folder,folder),'foreign':('linux:1:00000000000000ff','linux:1:00000000000000ff')}
        with contextlib.closing(sqlite3.connect(database)) as db,db:
            for name,volume in cases.items():
                db.execute("INSERT INTO removals VALUES (?,?,?,?,?,?,?,?,?)",(str(self.root/name),gone_repository,'b','0'*40,'f'*64,'',1.0,'partial',''))
                if volume:db.execute("INSERT INTO removal_volumes VALUES (?,?,?)",(str(self.root/name),*volume))
        listed=subprocess.run([str(HGS),'worktrees','leftovers','--json'],env=self.env,text=True,capture_output=True,timeout=60)
        self.assertEqual(listed.returncode,0,listed.stderr)
        paths=[path for path,_ in self.records()]
        # Gone for good: today's check for an older CLI's record, and a missing
        # folder and repository whose filesystem is still there.
        self.assertNotIn(str(self.root/'older-cli'),paths);self.assertNotIn(str(self.root/'same'),paths)
        # Uncertain, never gone: an unrecorded or another filesystem.
        self.assertIn(str(self.root/'unrecorded'),paths);self.assertIn(str(self.root/'foreign'),paths)
        self.assertEqual(sorted(p for p,_,_ in self.volumes()),sorted([str(tree.resolve()),str(self.root/'unrecorded'),str(self.root/'foreign')]))
        # The record a live removal settled keeps its volume row with it.
        shutil.rmtree(tree,ignore_errors=True)
        if tree.exists():(tree/'target'/'readonly').chmod(0o755);shutil.rmtree(tree)
        self.assertIsNone(self.leftover(tree));self.assertNotIn(str(tree.resolve()),[p for p,_,_ in self.volumes()])
    # #1206 review: the targeted filter matches whole paths, never a prefix,
    # and resolves a linked target to its record.
    def test_a_targeted_review_matches_whole_paths_and_resolved_links(self):
        pre,_,_=self.partial('pre');self.partial('pre-x')
        data=self.review(worktree=pre)
        self.assertEqual([row['path'] for row in data['leftovers']],[str(pre.resolve())])
        link=self.root/'link-to-pre';os.symlink(pre,link)
        data=self.review(worktree=link)
        self.assertEqual([row['path'] for row in data['leftovers']],[str(pre.resolve())])
    def test_an_interrupted_removal_stays_pending_for_review(self):
        self.fake("if any(sys.argv[i:i+2]==['worktree','remove'] for i in range(len(sys.argv)-1)):\n import signal,subprocess\n subprocess.run(["+repr(GIT)+",*sys.argv[1:]])\n os.kill(os.getppid(),signal.SIGKILL);sys.exit(1)\nelse: os.execv("+repr(GIT)+",["+repr(GIT)+",*sys.argv[1:]])\n")
        tree,row,result=self.partial('interrupted')
        self.assertEqual(result.returncode,-9,result.stderr)
        (self.home/'.local/bin/git').unlink()
        left=self.leftover(tree);self.assertIsNotNone(left)
        self.assertEqual(left['removal']['status'],'pending');self.assertEqual(left['verdict'],'partially_removed')
        self.assertEqual((tree/'target'/'readonly'/'out').read_text(),'build')
    # zerus-ty94 #6: hgs is killed before Git starts removing. The Git it
    # started inherited the repository lock, so the lock stays taken while that
    # orphan runs: a review must keep the pending record however old it looks,
    # and the folder Git leaves behind is still a reviewable leftover.
    def test_a_review_never_resolves_a_removal_an_orphaned_git_is_still_running(self):
        go=self.root/'orphan-go';done=self.root/'orphan-done'
        self.fake("args=sys.argv[1:]\n"
                  "if any(args[i:i+2]==['worktree','remove'] for i in range(len(args)-1)):\n"
                  " import signal,subprocess\n os.kill(os.getppid(),signal.SIGKILL)\n end=time.time()+30\n"
                  " while not os.path.exists("+repr(str(go))+") and time.time()<end: time.sleep(0.05)\n"
                  " r=subprocess.run(["+repr(GIT)+",*args],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL).returncode\n"
                  " open("+repr(str(done))+",'w').close();sys.exit(r)\n"
                  "os.execv("+repr(GIT)+",["+repr(GIT)+",*args])\n")
        tree,row,result=self.partial('orphaned-git')
        self.assertEqual(result.returncode,-9,result.stderr)
        self.assertEqual(self.records(),[(str(tree.resolve()),'pending')])
        # Git has not deregistered the worktree yet. A clock step or a long
        # Git run ages the record (review-w's probe): it must still stay.
        import sqlite3
        database=self.root/'state'/'worktree-maintenance'/'queue.sqlite3'
        with contextlib.closing(sqlite3.connect(database)) as db,db:db.execute("UPDATE removals SET reserved_at=reserved_at-100000")
        data=self.review();self.assertIn(str(tree.resolve()),[r['path'] for r in data['worktrees']])
        self.assertEqual(self.records(),[(str(tree.resolve()),'pending')])
        go.touch();end=time.time()+30
        while not done.exists() and time.time()<end:time.sleep(0.05)
        self.assertTrue(done.exists());(self.home/'.local/bin/git').unlink()
        left=self.leftover(tree);self.assertIsNotNone(left)
        self.assertEqual(left['removal']['status'],'pending')
        self.assertEqual((tree/'target'/'readonly'/'out').read_text(),'build')
    # Only the removal's Git inherits the repository lock; every other Git
    # hgs runs, before and after, never holds it, and it is free afterwards.
    @unittest.skipUnless(sys.platform=='linux','reads /proc/self/fd')
    def test_only_the_removal_git_inherits_the_repository_lock(self):
        log=self.root/'git-fds.log'
        self.fake("args=sys.argv[1:]\n"
                  "held=False\n"
                  "for name in os.listdir('/proc/self/fd'):\n"
                  " try:target=os.readlink('/proc/self/fd/'+name)\n"
                  " except OSError:continue\n"
                  " held=held or ('worktree-cache' in target and target.endswith('.lock'))\n"
                  "open("+repr(str(log))+",'a').write(repr((held,args))+'\\n')\n"
                  "os.execv("+repr(GIT)+",["+repr(GIT)+",*args])\n")
        clean=self.linked('inherits');self.age(clean);row=self.row(clean)
        result=self.remove(clean,row['fingerprint']);self.assertEqual(result.returncode,0,result.stderr)
        calls=[eval(line) for line in log.read_text().splitlines()]
        removal=[held for held,args in calls if any(args[i:i+2]==['worktree','remove'] for i in range(len(args)-1))]
        self.assertEqual(removal,[True])
        self.assertGreater(len(calls),len(removal));self.assertFalse(any(held for held,args in calls if 'remove' not in args))
        import fcntl,hashlib
        common=str((self.repo/'.git').resolve())
        path=self.root/'state'/'worktree-cache'/('repo-'+hashlib.sha256(common.encode()).hexdigest()+'.lock')
        with open(path,'r+') as handle:fcntl.flock(handle,fcntl.LOCK_EX|fcntl.LOCK_NB)
    # The removal releases its lock by closing it, never by an explicit
    # unlock: a helper the removal's Git started keeps it taken until it exits.
    @unittest.skipUnless(sys.platform in ('linux','darwin'),'flock inheritance')
    def test_the_repository_lock_lasts_while_anything_the_removal_git_started_runs(self):
        import fcntl,hashlib
        go=self.root/'helper-go'
        self.fake("args=sys.argv[1:]\n"
                  "if any(args[i:i+2]==['worktree','remove'] for i in range(len(args)-1)) and os.fork()==0:\n"
                  " [os.close(fd) for fd in (0,1,2)]\n end=time.time()+30\n"
                  " while not os.path.exists("+repr(str(go))+") and time.time()<end: time.sleep(0.05)\n"
                  " os._exit(0)\n"
                  "os.execv("+repr(GIT)+",["+repr(GIT)+",*args])\n")
        clean=self.linked('helper');self.age(clean);row=self.row(clean)
        result=self.remove(clean,row['fingerprint']);self.assertEqual(result.returncode,0,result.stderr)
        common=str((self.repo/'.git').resolve())
        path=self.root/'state'/'worktree-cache'/('repo-'+hashlib.sha256(common.encode()).hexdigest()+'.lock')
        def free():
            with open(path,'r+') as handle:
                try:fcntl.flock(handle,fcntl.LOCK_EX|fcntl.LOCK_NB);return True
                except BlockingIOError:return False
        self.assertFalse(free(),'the lock was released while a helper of the removal Git ran')
        go.touch();end=time.time()+10
        while not free() and time.time()<end:time.sleep(0.05)
        self.assertTrue(free())
    def test_resolved_and_stale_records_drop_and_the_bound_fails_closed(self):
        import sqlite3
        tree,_,_=self.partial('resolved');self.assertEqual(len(self.records()),1)
        (tree/'target'/'readonly').chmod(0o755);shutil.rmtree(tree)
        self.assertIsNone(self.leftover(tree));self.assertEqual(self.records(),[])
        # A pending record for a still-registered worktree adds nothing to its ordinary row.
        live=self.linked('live');database=self.root/'state'/'worktree-maintenance'/'queue.sqlite3'
        with contextlib.closing(sqlite3.connect(database)) as db,db:
            db.execute("INSERT INTO removals VALUES (?,?,?,?,?,?,?,?,?)",(str(live.resolve()),str((self.repo/'.git').resolve()),'feature/live','0'*40,'f'*64,'',1.0,'pending',''))
        self.assertIsNone(self.leftover(live));self.assertEqual(self.records(),[])
        # Records of a vanished repository whose folders still exist hold the bound.
        orphans=self.root/'orphans';orphans.mkdir()
        for n in range(512):(orphans/str(n)).mkdir()
        with contextlib.closing(sqlite3.connect(database)) as db,db:
            db.executemany("INSERT INTO removals VALUES (?,?,?,?,?,?,?,?,?)",[(str(orphans/str(n)),str(self.root/'gone-repository/.git'),'b','0'*40,'f'*64,'',1.0,'partial','') for n in range(512)])
        self.age(live);row=self.row(live)
        refused=self.remove(live,row['fingerprint']);self.assertNotEqual(refused.returncode,0)
        self.assertIn('512 earlier removals still need review',refused.stderr);self.assertEqual(json.loads(refused.stdout)['status'],'kept')
        self.assertTrue(live.exists());self.assertIn(str(live.resolve()),[r['path'] for r in self.review()['worktrees']])
        # Once their folders are gone too, nothing is left to review: they resolve.
        shutil.rmtree(orphans)
        result=self.remove(live,self.row(live)['fingerprint']);self.assertEqual(result.returncode,0,result.stderr)
        self.assertFalse(live.exists());self.assertEqual(self.records(),[])
    def test_capacity_refusal_commits_resolved_vanished_records(self):
        import sqlite3
        tree,_,_=self.partial('capacity-witness')
        live=self.linked('capacity-live');self.age(live);row=self.row(live)
        database=self.root/'state'/'worktree-maintenance'/'queue.sqlite3'
        # A late partial outcome can leave a historical store above the bound.
        # The first page contains a definitely vanished record, followed by
        # enough still-existing folders that the new removal must still refuse.
        vanished=self.root/'000-vanished';orphans=self.root/'capacity-orphans';orphans.mkdir()
        for n in range(512):(orphans/str(n)).mkdir()
        with contextlib.closing(sqlite3.connect(database)) as db,db:
            db.executemany("INSERT INTO removals VALUES (?,?,?,?,?,?,?,?,?)",
                [(str(vanished),str(self.root/'gone-repository/.git'),'b','0'*40,'f'*64,'',1.0,'partial','')]
                +[(str(orphans/str(n)),str(self.root/'gone-repository/.git'),'b','0'*40,'f'*64,'',1.0,'partial','') for n in range(512)])
        refused=self.remove(live,row['fingerprint'])
        self.assertNotEqual(refused.returncode,0);self.assertIn('512 earlier removals still need review',refused.stderr)
        self.assertEqual(json.loads(refused.stdout)['status'],'kept')
        remaining=dict(self.records())
        self.assertFalse(str(vanished) in remaining,'a refused reservation must not resurrect a verified resolution')
        self.assertEqual(len(remaining),513);self.assertIn(str(tree.resolve()),remaining)
        self.assertTrue(live.exists());self.assertEqual(len(list(orphans.iterdir())),512)
        self.assertIn('feature/capacity-live',self.git('-C',str(self.repo),'branch','--list','feature/capacity-live'))

    def test_a_stale_listing_never_forgets_a_partial_record(self):
        if os.geteuid()==0:self.skipTest('root ignores directory permissions')
        tree=self.linked('race');readonly=tree/'target'/'readonly';readonly.mkdir(parents=True);(readonly/'out').write_text('build')
        readonly.chmod(0o555);self.addCleanup(lambda:readonly.exists() and readonly.chmod(0o755))
        self.age(tree);row=self.row(tree)
        # Only the slow review's Git calls sleep, after its worktree list was taken.
        self.fake("args=sys.argv[1:]\n"
                  "if os.environ.get('REVIEW_SLOW') and 'symbolic-ref' in args: time.sleep(2.5)\n"
                  "os.execv("+repr(GIT)+",["+repr(GIT)+",*args])\n")
        slow=subprocess.Popen([str(HGS),'worktrees','review','--path',str(self.repo),'--json'],
                              env=dict(self.env,REVIEW_SLOW='1'),stdout=subprocess.PIPE,stderr=subprocess.PIPE,text=True)
        time.sleep(0.6)
        result=self.remove(tree,row['fingerprint'],extra=('--ignored-fingerprint',row['ignored_fingerprint']))
        self.assertIn('files remain',result.stderr);self.assertEqual([s for _,s in self.records()],['partial'])
        out,_=slow.communicate(timeout=60);json.loads(out)
        (self.home/'.local/bin/git').unlink()
        self.assertEqual([s for _,s in self.records()],['partial'])
        self.assertIsNotNone(self.leftover(tree),'the leftover folder exists but its record was forgotten')
        self.assertEqual((readonly/'out').read_text(),'build')
    def test_an_unreadable_list_after_git_keeps_the_record_pending(self):
        marker=self.root/'removed-marker'
        self.fake("args=sys.argv[1:]\n"
                  "if any(args[i:i+2]==['worktree','remove'] for i in range(len(args)-1)):\n"
                  " import subprocess\n r=subprocess.run(["+repr(GIT)+",*args]).returncode\n open("+repr(str(marker))+",'w').close();sys.exit(r)\n"
                  "if os.path.exists("+repr(str(marker))+") and any(args[i:i+2]==['worktree','list'] for i in range(len(args)-1)):\n"
                  " sys.stderr.write('fatal: synthetic unreadable list\\n');sys.exit(128)\n"
                  "os.execv("+repr(GIT)+",["+repr(GIT)+",*args])\n")
        tree,row,result=self.partial('unknown')
        self.assertNotEqual(result.returncode,0,result.stderr);self.assertTrue(marker.exists())
        self.assertIn('could not be confirmed',result.stderr)
        (self.home/'.local/bin/git').unlink()
        self.assertEqual([s for _,s in self.records()],['pending'])
        left=self.leftover(tree);self.assertIsNotNone(left);self.assertEqual(left['removal']['status'],'pending')
        self.assertEqual((tree/'target'/'readonly'/'out').read_text(),'build')
    def test_unreadable_folder_metadata_after_git_is_never_taken_as_removal(self):
        if os.geteuid()==0:self.skipTest('root ignores directory permissions')
        box=self.root/'box';box.mkdir();tree=box/'tree'
        self.git('-C',str(self.repo),'worktree','add','-qb','feature/boxed',str(tree))
        readonly=tree/'target'/'readonly';readonly.mkdir(parents=True);(readonly/'out').write_text('build')
        readonly.chmod(0o555);self.addCleanup(lambda:box.chmod(0o755) or (readonly.exists() and readonly.chmod(0o755)))
        self.age(tree);row=self.row(tree)
        # After Git ran, the folder's parent cannot be searched: its metadata is unreadable.
        self.fake("args=sys.argv[1:]\n"
                  "if any(args[i:i+2]==['worktree','remove'] for i in range(len(args)-1)):\n"
                  " import subprocess\n r=subprocess.run(["+repr(GIT)+",*args]).returncode\n os.chmod("+repr(str(box))+",0o600);sys.exit(r)\n"
                  "os.execv("+repr(GIT)+",["+repr(GIT)+",*args])\n")
        result=self.remove(tree,row['fingerprint'],extra=('--ignored-fingerprint',row['ignored_fingerprint']))
        box.chmod(0o755);(self.home/'.local/bin/git').unlink()
        self.assertNotEqual(result.returncode,0);self.assertEqual(json.loads(result.stdout)['status'],'unconfirmed')
        self.assertEqual(self.records(),[(str(tree.resolve()),'pending')])
        left=self.leftover(tree);self.assertIsNotNone(left);self.assertEqual(left['removal']['status'],'pending')
    def test_a_leftover_of_a_vanished_repository_stays_reachable(self):
        tree,row,_=self.partial('orphaned');left=self.leftover(tree);common=left['common_dir']
        shutil.rmtree(self.repo)
        listed=subprocess.run([str(HGS),'worktrees','leftovers','--json'],env=self.env,text=True,capture_output=True,timeout=30)
        self.assertEqual(listed.returncode,0,listed.stderr);rows=json.loads(listed.stdout)['leftovers']
        self.assertEqual([r['path'] for r in rows],[str(tree.resolve())]);orphan=rows[0]
        self.assertFalse(orphan['repository_available']);self.assertIsNone(orphan['branch_retained'])
        keep=lambda fp:subprocess.run([str(HGS),'worktrees','keep-folder','--path',str(tree),'--common-dir',common,'--fingerprint',fp,'--json'],
            env=self.env,text=True,capture_output=True,timeout=30)
        self.assertIn('changed since the review',keep('0'*64).stderr)
        kept=keep(orphan['fingerprint']);self.assertEqual(kept.returncode,0,kept.stderr);self.assertEqual(json.loads(kept.stdout)['status'],'kept')
        self.assertEqual((tree/'target'/'readonly'/'out').read_text(),'build');self.assertEqual(self.records(),[])
    def test_many_leftovers_stay_bounded_and_the_catalog_counts_them(self):
        import sqlite3
        tree,_,_=self.partial('counted')
        self.assertEqual(self.catalog()['leftovers'],1)
        data=self.review();self.assertEqual(len(data['leftovers']),1);self.assertFalse(data['leftovers_partial'])
        database=self.root/'state'/'worktree-maintenance'/'queue.sqlite3';common=str((self.repo/'.git').resolve())
        extra=self.root/'extra';gone=self.root/'gone'
        for n in range(40):(extra/str(n)).mkdir(parents=True);(gone/str(n)).mkdir(parents=True)
        with contextlib.closing(sqlite3.connect(database)) as db,db:
            db.executemany("INSERT INTO removals VALUES (?,?,?,?,?,?,?,?,?)",
                [(str(extra/str(n)),common,'b','0'*40,'f'*64,'',1.0,'partial','') for n in range(40)]
                +[(str(gone/str(n)),str(self.root/'gone-repository/.git'),'b','0'*40,'f'*64,'',1.0,'partial','') for n in range(40)])
        self.assertEqual(self.catalog()['leftovers'],41)
        data=self.review();self.assertEqual(len(data['leftovers']),32);self.assertTrue(data['leftovers_partial'])
        listed=subprocess.run([str(HGS),'worktrees','leftovers','--json'],env=self.env,text=True,capture_output=True,timeout=60)
        self.assertEqual(listed.returncode,0,listed.stderr);value=json.loads(listed.stdout)
        self.assertEqual(len(value['leftovers']),32);self.assertTrue(value['partial'])
        # Nothing beyond the bound is forgotten; vanished folders still resolve.
        self.assertEqual(len(self.records()),81)
        shutil.rmtree(extra)
        data=self.review();self.assertEqual([r['path'] for r in data['leftovers']],[str(tree.resolve())])
        self.assertFalse(data['leftovers_partial']);self.assertEqual(self.catalog()['leftovers'],1)
    def test_leftover_rows_stay_within_their_byte_bound(self):
        import sqlite3
        tree,_,_=self.partial('large');database=self.root/'state'/'worktree-maintenance'/'queue.sqlite3'
        common=str((self.repo/'.git').resolve());extra=self.root/'large-details';gone=self.root/'large-gone'
        for n in range(20):(extra/str(n)).mkdir(parents=True);(gone/str(n)).mkdir(parents=True)
        # Twenty rows of about 100 KB each: the 32-row bound alone admits all of them.
        with contextlib.closing(sqlite3.connect(database)) as db,db:
            db.executemany("INSERT INTO removals VALUES (?,?,?,?,?,?,?,?,?)",
                [(str(extra/str(n)),common,'b','0'*40,'f'*64,'',1.0,'partial','x'*100_000) for n in range(20)]
                +[(str(gone/str(n)),str(self.root/'gone-repository/.git'),'b','0'*40,'f'*64,'',1.0,'partial','x'*100_000) for n in range(20)])
        data=self.review();rows=data['leftovers']
        self.assertTrue(data['leftovers_partial']);self.assertLess(len(rows),21);self.assertGreater(len(rows),1)
        self.assertLessEqual(len(json.dumps(rows,separators=(',',':')).encode()),1024*1024)
        listed=subprocess.run([str(HGS),'worktrees','leftovers','--json'],env=self.env,text=True,capture_output=True,timeout=60)
        self.assertEqual(listed.returncode,0,listed.stderr);value=json.loads(listed.stdout)
        self.assertTrue(value['partial']);self.assertLess(len(value['leftovers']),20)
        self.assertLessEqual(len(json.dumps(value['leftovers'],separators=(',',':')).encode()),1024*1024)
        self.assertEqual(len(self.records()),41)
    def delete(self,path,fingerprint,confirm=True,extra=()):
        return subprocess.run([str(HGS),'worktrees','delete-leftover','--path',str(path),'--common-dir',str(self.repo/'.git'),'--fingerprint',fingerprint,
            *(['--confirm-delete'] if confirm else []),'--request-id','test-delete','--json',*extra],env=self.env,text=True,capture_output=True,timeout=60)
    @unittest.skipUnless(sys.platform == 'darwin', 'macOS fail-closed leftover contract')
    def test_macos_leftover_refusal_preserves_files_permissions_record_and_keep_folder(self):
        import sqlite3
        tree,row,_=self.partial('macos-refused');left=self.leftover(tree)
        self.assertIn('macos_appledouble_safety',codes(left['reasons']))
        self.assertFalse(left['deletable']);self.assertNotIn('process_fingerprint',left)
        payload=tree/'target'/'readonly'/'out';mode=payload.parent.stat().st_mode
        database=self.root/'state'/'worktree-maintenance'/'queue.sqlite3'
        with contextlib.closing(sqlite3.connect(database)) as db:
            before=db.execute('SELECT * FROM removals').fetchall()
        result=self.delete(tree,left['fingerprint'])
        self.assertNotEqual(result.returncode,0);value=json.loads(result.stdout)
        self.assertEqual((value['operation'],value['outcome_version'],value['status']),('worktree_leftover_delete',1,'kept'))
        self.assertEqual(value['request_id'],'test-delete');self.assertIn('AppleDouble',value['detail']);self.assertIn('Finder',value['detail'])
        self.assertEqual(payload.read_text(),'build');self.assertEqual(payload.parent.stat().st_mode,mode)
        with contextlib.closing(sqlite3.connect(database)) as db:
            self.assertEqual(db.execute('SELECT * FROM removals').fetchall(),before,'no attempt marker may be written')
        self.assertEqual(self.leftover(tree)['fingerprint'],left['fingerprint'])
        self.assertIn(row['branch'],self.git('-C',str(self.repo),'branch','--list',row['branch']))
        kept=self.keep(tree,left['fingerprint']);self.assertEqual(kept.returncode,0,kept.stderr)
        self.assertEqual(payload.read_text(),'build');self.assertEqual(self.records(),[])

    @unittest.skipUnless(sys.platform == 'linux', 'Linux protected-process review')
    def test_unrelated_hidden_process_churn_keeps_leftover_confirmations_valid(self):
        def hidden():
            child=subprocess.Popen([sys.executable,'-I','-c',
                'import ctypes,time; assert ctypes.CDLL(None).prctl(4,0,0,0,0)==0; print("ready",flush=True); time.sleep(90)'],
                cwd=self.root,stdout=subprocess.PIPE,text=True)
            self.addCleanup(child.stdout.close);self.addCleanup(child.wait);self.addCleanup(lambda: child.poll() is None and child.kill())
            self.assertEqual(child.stdout.readline().strip(),'ready')
        kept,_,_=self.partial('kept-churn');deleted,_,_=self.partial('deleted-churn')
        hidden();reviewed=self.review()
        keep_row=self.leftover(kept,reviewed);delete_row=self.leftover(deleted,reviewed)
        self.assertEqual(codes(delete_row['reasons']),['processes_unavailable']);self.assertTrue(delete_row['process_fingerprint'])
        # An unrelated hidden process starts before the owner confirms.
        hidden()
        self.assertEqual(self.leftover(deleted)['fingerprint'],delete_row['fingerprint'])
        kept_result=self.keep(kept,keep_row['fingerprint']);self.assertEqual(kept_result.returncode,0,kept_result.stderr)
        result=self.delete(deleted,delete_row['fingerprint'],extra=('--process-fingerprint',delete_row['process_fingerprint']))
        self.assertEqual(result.returncode,0,result.stderr);self.assertFalse(deleted.exists());self.assertTrue(kept.exists())
    @unittest.skipUnless(sys.platform == 'linux', 'Leftover deletion is disabled on macOS pending a native AppleDouble guard')
    def test_deleting_a_leftover_needs_confirmation_and_a_fresh_review_and_keeps_the_branch(self):
        tree,row,_=self.partial('deleted');outside=self.root/'outside';outside.mkdir();(outside/'keep.txt').write_text('outside')
        (tree/'target'/'readonly').chmod(0o755);(tree/'target'/'readonly'/'link').symlink_to(outside,target_is_directory=True);(tree/'target'/'readonly').chmod(0o555)
        left=self.leftover(tree);self.assertTrue(left['deletable'],left)
        unconfirmed=self.delete(tree,left['fingerprint'],confirm=False);self.assertNotEqual(unconfirmed.returncode,0)
        self.assertIn('--confirm-delete',unconfirmed.stderr);self.assertEqual(json.loads(unconfirmed.stdout)['status'],'kept')
        stale=self.delete(tree,'0'*64);self.assertNotEqual(stale.returncode,0);self.assertIn('changed since the review',stale.stderr)
        self.assertEqual((tree/'target'/'readonly'/'out').read_text(),'build')
        deleted=self.delete(tree,left['fingerprint']);self.assertEqual(deleted.returncode,0,deleted.stderr)
        value=json.loads(deleted.stdout);self.assertEqual((value['status'],value['request_id'],value['branch']),('deleted','test-delete',row['branch']))
        self.assertFalse(tree.exists());self.assertEqual((outside/'keep.txt').read_text(),'outside')
        self.assertEqual(self.records(),[]);self.assertIsNone(self.leftover(tree))
        self.assertIn(row['branch'],self.git('-C',str(self.repo),'branch','--list',row['branch']))
    @unittest.skipUnless(sys.platform == 'linux', 'Leftover deletion is disabled on macOS pending a native AppleDouble guard')
    def test_deleting_a_leftover_refuses_sessions_processes_and_nested_repositories(self):
        tree,_,_=self.partial('occupied')
        self.binding('codex/fixture/leftover',tree);left=self.leftover(tree)
        self.assertFalse(left['deletable']);self.assertIn('sessions',codes(left['reasons']))
        refused=self.delete(tree,left['fingerprint']);self.assertNotEqual(refused.returncode,0);self.assertIn('Archive them first',refused.stderr)
        self.assertEqual(json.loads(refused.stdout)['status'],'kept')
        for file in (self.root/'state').glob('*.json'):file.unlink()
        sleeper=subprocess.Popen(['sleep','60'],cwd=tree);self.addCleanup(sleeper.wait);self.addCleanup(sleeper.kill)
        left=self.leftover(tree);self.assertIn('processes',codes(left['reasons']))
        self.assertNotEqual(self.delete(tree,left['fingerprint']).returncode,0)
        sleeper.kill();sleeper.wait()
        (tree/'target'/'readonly').chmod(0o755);nested=tree/'target'/'readonly'/'inner';nested.mkdir();self.git('init','-q',str(nested));(tree/'target'/'readonly').chmod(0o555)
        left=self.leftover(tree);self.assertIn('nested_repository',codes(left['reasons']));self.assertFalse(left['deletable'])
        self.assertNotEqual(self.delete(tree,left['fingerprint']).returncode,0)
        self.assertTrue((nested/'.git').is_dir());self.assertEqual((tree/'target'/'readonly'/'out').read_text(),'build')
    @unittest.skipUnless(sys.platform == 'linux', 'Leftover deletion is disabled on macOS pending a native AppleDouble guard')
    def test_leftover_deletion_refuses_an_incomplete_saved_binding_inventory(self):
        for failure in ('unreadable-file','malformed-record','unreadable-directory'):
            with self.subTest(failure=failure):
                tree,row,_=self.partial('binding-'+failure)
                name='codex/protected-'+failure;self.binding(name,tree)
                known=self.root/'state'/(hashlib.sha256(name.encode()).hexdigest()+'.json')
                self.assertIn('sessions',codes(self.leftover(tree)['reasons']))
                unrelated='codex/unrelated-'+failure;self.binding(unrelated,self.root)
                bad=self.root/'state'/(hashlib.sha256(unrelated.encode()).hexdigest()+'.json')
                original=bad.read_text()
                directory=self.root/'state'/'bindings';directory.mkdir(exist_ok=True)
                if failure=='unreadable-file':bad.chmod(0)
                elif failure=='malformed-record':bad.write_text('{invalid')
                else:directory.chmod(0)
                try:
                    current=self.leftover(tree)
                    # Use the fresh proof: refusal cannot rely on an old one
                    # becoming stale, nor on the known record being unreadable.
                    result=self.delete(tree,current['fingerprint'])
                    self.assertNotEqual(result.returncode,0,result.stdout)
                    self.assertIn('Saved sessions could not be checked',result.stderr)
                    self.assertEqual(tuple(json.loads(result.stdout)[key] for key in ('operation','outcome_version','status')),('worktree_leftover_delete',1,'kept'))
                    self.assertFalse(current['deletable'])
                    self.assertIn('sessions_unavailable',codes(current['reasons']))
                    self.assertFalse(current.get('process_fingerprint'))
                    self.assertEqual((tree/'target'/'readonly'/'out').read_text(),'build')
                    self.assertIn(row['branch'],self.git('-C',str(self.repo),'branch','--list',row['branch']))
                finally:
                    bad.chmod(0o600);bad.write_text(original);directory.chmod(0o700)
                # A repaired inventory still protects the real saved reference.
                repaired=self.leftover(tree)
                self.assertIn('sessions',codes(repaired['reasons']));self.assertFalse(repaired['deletable'])
                known.unlink();bad.unlink()
                ready=self.leftover(tree);self.assertTrue(ready['deletable'])
                allowed=self.delete(tree,ready['fingerprint']);self.assertEqual(allowed.returncode,0,allowed.stderr)
                self.assertFalse(tree.exists())

    @unittest.skipUnless(sys.platform == 'linux', 'Linux protected-process review')
    def test_process_only_consent_never_overrides_an_unreadable_binding_inventory(self):
        tree,_,_=self.partial('binding-consent')
        child=subprocess.Popen([sys.executable,'-I','-c',
            'import ctypes,time; assert ctypes.CDLL(None).prctl(4,0,0,0,0)==0; print("ready",flush=True); time.sleep(90)'],
            cwd=self.root,stdout=subprocess.PIPE,text=True)
        self.addCleanup(child.stdout.close);self.addCleanup(child.wait);self.addCleanup(lambda:child.poll() is None and child.kill())
        self.assertEqual(child.stdout.readline().strip(),'ready')
        before=self.leftover(tree)
        self.assertEqual(codes(before['reasons']),['processes_unavailable'])
        self.assertTrue(before['process_fingerprint'])
        name='codex/unreadable-consent';self.binding(name,self.root)
        bad=self.root/'state'/(hashlib.sha256(name.encode()).hexdigest()+'.json');bad.chmod(0)
        try:
            current=self.leftover(tree)
            result=self.delete(tree,current['fingerprint'],extra=('--process-fingerprint',before['process_fingerprint']))
            self.assertNotEqual(result.returncode,0,result.stdout)
            self.assertEqual(tuple(json.loads(result.stdout)[key] for key in ('operation','outcome_version','status')),('worktree_leftover_delete',1,'kept'))
            self.assertIn('sessions_unavailable',codes(current['reasons']))
            self.assertIn('processes_unavailable',codes(current['reasons']))
            self.assertFalse(current.get('process_fingerprint'))
            self.assertEqual((tree/'target'/'readonly'/'out').read_text(),'build')
        finally:bad.chmod(0o600)

    @unittest.skipUnless(sys.platform == 'linux', 'Leftover deletion is disabled on macOS pending a native AppleDouble guard')
    def test_a_stopped_deletion_keeps_its_record_and_is_never_retried(self):
        tree,_,_=self.partial('stopped');left=self.leftover(tree)
        parent=tree.parent;mode=parent.stat().st_mode;parent.chmod(0o555);self.addCleanup(lambda:parent.chmod(mode))
        stopped=self.delete(tree,left['fingerprint']);parent.chmod(mode)
        self.assertNotEqual(stopped.returncode,0);self.assertIn('Deleting the leftover stopped',stopped.stderr)
        self.assertEqual(json.loads(stopped.stdout)['status'],'unconfirmed')
        self.assertTrue(tree.exists());self.assertFalse((tree/'target').exists())
        later=self.leftover(tree);self.assertIn('Deleting the leftover stopped',later['removal']['detail'])
        self.assertEqual(later['removal']['status'],'partial');self.assertNotEqual(later['fingerprint'],left['fingerprint'])
        self.assertNotEqual(self.delete(tree,left['fingerprint']).returncode,0);self.assertTrue(tree.exists())
        finished=self.delete(tree,later['fingerprint']);self.assertEqual(finished.returncode,0,finished.stderr)
        self.assertFalse(tree.exists());self.assertEqual(self.records(),[])
    @unittest.skipUnless(sys.platform == 'linux', 'Leftover deletion is disabled on macOS pending a native AppleDouble guard')
    def test_delete_confirmation_is_spent_even_when_no_file_changed_and_settlement_fails(self):
        import sqlite3
        tree,row,_=self.partial('single-use')
        retained=self.root/'retained-ignored-payload';tree.rename(retained);tree.mkdir()
        left=self.leftover(tree);identity=(tree.stat().st_ino,tree.stat().st_mtime_ns)
        database=self.root/'state'/'worktree-maintenance'/'queue.sqlite3'
        with contextlib.closing(sqlite3.connect(database)) as db,db:
            db.execute("CREATE TRIGGER synthetic_settle_failure BEFORE INSERT ON removals WHEN NEW.detail LIKE 'Deleting the leftover stopped:%' BEGIN SELECT RAISE(ABORT,'synthetic settlement failure'); END")
        mode=self.root.stat().st_mode
        def stopped(proof):
            self.root.chmod(0o555)
            try:result=self.delete(tree,proof)
            finally:self.root.chmod(mode)
            self.assertNotEqual(result.returncode,0);self.assertIn('Deleting the leftover stopped',result.stderr)
            value=json.loads(result.stdout)
            self.assertEqual((value['operation'],value['outcome_version'],value['status']),('worktree_leftover_delete',1,'unconfirmed'))
            self.assertEqual((tree.stat().st_ino,tree.stat().st_mtime_ns),identity)
            self.assertEqual(list(tree.iterdir()),[])
        stopped(left['fingerprint'])
        later=self.leftover(tree)
        self.assertNotEqual(later['fingerprint'],left['fingerprint'],'the pre-walk record must consume the confirmation even when settlement fails')
        stale=self.delete(tree,left['fingerprint']);self.assertNotEqual(stale.returncode,0)
        self.assertEqual(json.loads(stale.stdout)['status'],'kept');self.assertTrue(tree.exists())
        with contextlib.closing(sqlite3.connect(database)) as db,db:db.execute('DROP TRIGGER synthetic_settle_failure')
        # Repeating the same refusal must consume a different confirmation too.
        for _ in range(2):
            before=self.leftover(tree);stopped(before['fingerprint'])
            self.assertNotEqual(self.leftover(tree)['fingerprint'],before['fingerprint'],'a repeated refusal must not restore its spent confirmation')
        self.assertEqual((retained/'target'/'readonly'/'out').read_text(),'build')
        self.assertIn(row['branch'],self.git('-C',str(self.repo),'branch','--list',row['branch']))
    @unittest.skipUnless(sys.platform == 'linux', 'Leftover deletion is disabled on macOS pending a native AppleDouble guard')
    def test_unwritable_attempt_record_refuses_before_deleting_any_files(self):
        import sqlite3
        tree,_,_=self.partial('intent-refused');left=self.leftover(tree)
        database=self.root/'state'/'worktree-maintenance'/'queue.sqlite3'
        with contextlib.closing(sqlite3.connect(database)) as db,db:
            for operation in ('INSERT','UPDATE'):
                db.execute(f"CREATE TRIGGER synthetic_intent_{operation.lower()} BEFORE {operation} ON removals BEGIN SELECT RAISE(ABORT,'synthetic intent failure'); END")
        result=self.delete(tree,left['fingerprint']);self.assertNotEqual(result.returncode,0)
        value=json.loads(result.stdout)
        self.assertEqual((value['operation'],value['outcome_version'],value['status']),('worktree_leftover_delete',1,'kept'))
        self.assertIn('synthetic intent failure',value['detail'])
        self.assertEqual((tree/'target'/'readonly'/'out').read_text(),'build')
        self.assertEqual(self.leftover(tree)['fingerprint'],left['fingerprint'])

    def inspect(self,path,file=None,common=None):
        return subprocess.run([str(HGS),'worktrees','inspect','--path',str(path),'--common-dir',str(common or self.repo/'.git'),*(['--file',file] if file else []),'--json'],env=self.env,text=True,capture_output=True,timeout=30)
    def test_ignored_notes_are_not_candidates_and_require_exact_consent(self):
        tree=self.linked('local-data');(tree/'artifacts').mkdir();notes=tree/'artifacts'/'research.txt';notes.write_text('important synthetic notes');self.age(tree)
        row=self.row(tree);self.assertEqual(row['verdict'],'review');self.assertIn('ignored_data',codes(row['notes']))
        refused=self.remove(tree,row['fingerprint']);self.assertNotEqual(refused.returncode,0);self.assertIn('explicitly confirm',refused.stderr);self.assertTrue(notes.exists())
        notes.write_text('changed important synthetic notes')
        stale=self.remove(tree,row['fingerprint'],extra=('--ignored-fingerprint',row['ignored_fingerprint']))
        self.assertNotEqual(stale.returncode,0);self.assertIn('changed since',stale.stderr);self.assertTrue(notes.exists())
        current=self.row(tree);allowed=self.remove(tree,current['fingerprint'],extra=('--ignored-fingerprint',current['ignored_fingerprint']))
        self.assertEqual(allowed.returncode,0,allowed.stderr);self.assertFalse(tree.exists())
    def test_equivalent_content_explains_squash_but_keeps_original_branch(self):
        tree=self.linked('squashed');(tree/'result.txt').write_text('finished feature');self.commit(tree,'original feature',add=True)
        (self.repo/'result.txt').write_text('finished feature');self.commit(self.repo,'squash integration',add=True);self.age(tree)
        row=self.row(tree);self.assertFalse(row['merged']);self.assertEqual(row['integration']['kind'],'same_tree');self.assertEqual(row['integration']['base'],'main');self.assertEqual(row['verdict'],'ready')
        result=self.remove(tree,row['fingerprint'],extra=('--delete-branch',));self.assertEqual(result.returncode,0,result.stderr);self.assertFalse(json.loads(result.stdout)['branch_deleted'])
        self.assertTrue(self.git('-C',str(self.repo),'branch','--list','feature/squashed'))
    def test_cherry_pick_equivalence_survives_later_base_work(self):
        tree=self.linked('picked');(tree/'feature.txt').write_text('feature');self.commit(tree,'original',add=True)
        commit=self.git('-C',str(tree),'rev-parse','HEAD')
        (self.repo/'other.txt').write_text('base change');self.commit(self.repo,'unrelated',add=True)
        self.git('-C',str(self.repo),'-c','user.name=Fixture','-c','user.email=fixture@example.test','cherry-pick',commit);self.age(tree)
        row=self.row(tree);self.assertFalse(row['merged']);self.assertEqual(row['integration']['kind'],'equivalent_patches');self.assertEqual(row['verdict'],'ready')
    def test_inspection_reads_actual_changes_commits_and_local_files(self):
        tree=self.linked('inspect');(tree/'committed.txt').write_text('useful feature');self.commit(tree,'valuable work',add=True)
        (tree/'committed.txt').write_text('unsaved change');(tree/'new.txt').write_text('untracked notes');(tree/'artifacts').mkdir();(tree/'artifacts'/'notes.txt').write_text('ignored research')
        result=self.inspect(tree);self.assertEqual(result.returncode,0,result.stderr);data=json.loads(result.stdout)
        self.assertEqual(data['path'],str(tree.resolve()));self.assertEqual(data['common_dir'],str((self.repo/'.git').resolve()));self.assertIn('unsaved change',data['changes']['text']);self.assertIn('valuable work',data['commits']['text'])
        self.assertEqual({f['path'] for f in data['files']},{'committed.txt','new.txt'})
        folder=json.loads(self.inspect(tree,'artifacts').stdout);self.assertEqual(folder['file']['entries'][0]['path'],'artifacts/notes.txt')
        preview=json.loads(self.inspect(tree,'artifacts/notes.txt').stdout);self.assertEqual(preview['file']['text'],'ignored research')
        self.assertEqual((tree/'committed.txt').read_text(),'unsaved change');self.assertTrue((tree/'new.txt').exists())
    def test_inspection_refuses_escape_symlinks_and_wrong_repository(self):
        tree=self.linked('inspect-guard');outside=self.root/'private.txt';outside.write_text('outside fixture data')
        (tree/'link').symlink_to(outside);(tree/'directory-link').symlink_to(self.root,target_is_directory=True)
        for name in ('../private.txt','link','directory-link/private.txt','/etc/passwd'):
            with self.subTest(name=name):
                result=self.inspect(tree,name);self.assertNotEqual(result.returncode,0);self.assertNotIn('outside fixture data',result.stdout)
        other=self.root/'other-repo';self.git('init','-q',str(other))
        self.assertNotEqual(self.inspect(tree,common=other/'.git').returncode,0)
    def test_inspection_bounds_text_and_marks_binary(self):
        tree=self.linked('inspect-limits');(tree/'large.txt').write_bytes(b'x'*(200*1024));(tree/'binary').write_bytes(b'one\x00two')
        large=json.loads(self.inspect(tree,'large.txt').stdout)['file'];self.assertTrue(large['truncated']);self.assertLessEqual(len(large['text']),128*1024)
        binary=json.loads(self.inspect(tree,'binary').stdout)['file'];self.assertTrue(binary['binary']);self.assertEqual(binary['text'],'')
    def test_inspection_pages_directory_and_rejects_special_files(self):
        tree=self.linked('inspect-pages');directory=tree/'artifacts';directory.mkdir()
        for i in range(205):(directory/f'notes-{i}.txt').write_text(f'notes {i}')
        first=json.loads(self.inspect(tree,'artifacts').stdout)['file'];self.assertEqual(len(first['entries']),200);self.assertEqual(first['next_offset'],200)
        result=subprocess.run([str(HGS),'worktrees','inspect','--path',str(tree),'--common-dir',str(self.repo/'.git'),'--file','artifacts','--offset','200','--json'],env=self.env,capture_output=True,text=True,timeout=8)
        self.assertEqual(result.returncode,0,result.stderr);second=json.loads(result.stdout)['file'];self.assertEqual(len(second['entries']),5);self.assertIsNone(second['next_offset'])
        self.assertEqual(len({e['path'] for e in first['entries']+second['entries']}),205)
        root=json.loads(self.inspect(tree,'.').stdout)['file'];self.assertIn('artifacts',[e['path'] for e in root['entries']])
        if hasattr(os,'mkfifo'):
            os.mkfifo(tree/'pipe');result=self.inspect(tree,'pipe');self.assertNotEqual(result.returncode,0);self.assertIn('ordinary files',result.stderr)
    def test_forget_missing_checkout_keeps_branch_and_locks(self):
        gone=self.linked('gone');shutil.rmtree(gone);row=self.row(gone);self.assertEqual(row['verdict'],'missing')
        result=self.remove(gone,row['fingerprint']);self.assertEqual(result.returncode,0,result.stderr);self.assertEqual(json.loads(result.stdout)['status'],'forgotten')
        self.assertFalse((self.repo/'.git/worktrees/gone').exists());self.assertIn('feature/gone',self.git('-C',str(self.repo),'branch','--list','feature/gone'))
        locked=self.linked('locked');self.git('-C',str(self.repo),'worktree','lock',str(locked));shutil.rmtree(locked)
        row=self.row(locked);self.assertEqual(row['verdict'],'blocked');self.assertNotEqual(self.remove(locked,row['fingerprint']).returncode,0)
        self.assertTrue((self.repo/'.git/worktrees/locked').exists())

    def test_manual_discard_requires_exact_content_and_preserves_unmerged_branch(self):
        tree=self.linked('discard');(tree/'work.txt').write_text('committed');self.commit(tree,'unmerged work',add=True)
        (tree/'work.txt').write_text('modified');(tree/'notes').mkdir();(tree/'notes/private.txt').write_text('local notes')
        row=self.row(tree);self.assertEqual(row['verdict'],'blocked');self.assertTrue(row['discardable'])
        self.assertNotEqual(self.remove(tree,row['fingerprint']).returncode,0)
        before=(tree/'notes/private.txt').stat();(tree/'notes/private.txt').write_text('other notes');os.utime(tree/'notes/private.txt',ns=(before.st_atime_ns,before.st_mtime_ns))
        stale=self.remove(tree,row['fingerprint'],extra=('--discard-fingerprint',row['discard_fingerprint']));self.assertNotEqual(stale.returncode,0);self.assertTrue(tree.exists())
        row=self.row(tree);result=self.remove(tree,row['fingerprint'],extra=('--discard-fingerprint',row['discard_fingerprint'],'--delete-branch'))
        self.assertEqual(result.returncode,0,result.stderr);self.assertFalse(tree.exists());self.assertFalse(json.loads(result.stdout)['branch_deleted'])
        self.assertTrue(self.git('-C',str(self.repo),'show-ref','refs/heads/feature/discard'))

    def test_manual_discard_cannot_override_local_consent_or_hard_blockers(self):
        tree=self.linked('protected');(tree/'untracked.txt').write_text('keep');(tree/'target').mkdir();(tree/'target/notes').write_text('keep too')
        row=self.row(tree);self.assertTrue(row['discardable'])
        self.assertNotEqual(self.remove(tree,row['fingerprint'],extra=('--discard-fingerprint',row['discard_fingerprint'])).returncode,0)
        self.git('-C',str(self.repo),'worktree','lock',str(tree));locked=self.row(tree);self.assertFalse(locked['discardable'])
        self.assertNotEqual(self.remove(tree,locked['fingerprint'],extra=('--discard-fingerprint',row['discard_fingerprint'],'--ignored-fingerprint',row['ignored_fingerprint'])).returncode,0)
        self.assertTrue((tree/'untracked.txt').exists())

    def test_discard_refuses_unbounded_or_special_files_and_staged_changes_invalidate_proof(self):
        tree=self.linked('proof');file=tree/'data';file.write_text('first');self.git('-C',str(tree),'add','data')
        row=self.row(tree);self.assertTrue(row['discardable']);file.write_text('other');self.git('-C',str(tree),'add','data');file.write_text('first')
        self.assertNotEqual(self.row(tree)['discard_fingerprint'],row['discard_fingerprint'])
        with open(tree/'large','wb') as stream:stream.truncate(65*1024*1024)
        self.assertFalse(self.row(tree)['discardable']);(tree/'large').unlink()
        if hasattr(os,'mkfifo'):
            os.mkfifo(tree/'pipe');self.assertFalse(self.row(tree)['discardable'])

    def test_unreadable_binding_and_ignored_nested_repository_never_authorize_discard(self):
        tree=self.linked('inventory');(tree/'notes.txt').write_text('keep');(tree/'artifacts').mkdir()
        self.git('init','-q',str(tree/'artifacts/nested'))
        row=self.row(tree);self.assertIn('nested_repository',codes(row['reasons']));self.assertFalse(row['discardable'])
        shutil.rmtree(tree/'artifacts');state=self.root/'state';state.mkdir(exist_ok=True);broken=state/('f'*64+'.json');broken.write_text('{broken')
        row=self.row(tree);self.assertIn('sessions_unavailable',codes(row['reasons']));self.assertFalse(row['discardable'])

if __name__=='__main__':unittest.main()
