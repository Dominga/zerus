"""Managed cleanup and archive restoration against private Git/tmux/fake agents."""
import json
import os
from pathlib import Path
import select
import shutil
import subprocess
import time
import unittest
import uuid

import test_pause

test_pause.HGS=Path(os.environ.get('HGS_TEST_BIN',Path(__file__).resolve().parents[1]/'target/debug/hgs')).resolve()


@unittest.skipUnless(test_pause.TMUX and shutil.which('git'), 'private tmux and Git required')
class WorktreeLifecycle(test_pause.Harness):
    def setUp(self):
        super().setUp();self.repo=self.project
        self.git('init','-q','-b','main',str(self.repo))
        (self.repo/'tracked').write_text('original')
        self.git('-C',str(self.repo),'add','tracked');self.commit(self.repo,'initial')
        self.project=self.root/'managed tree'
        self.hgs('worktrees','create','--path',str(self.repo),'--destination',str(self.project),'--branch','session-work','--json')
        (self.root/'cfg/projects.local').write_text('p='+str(self.project)+'\n')

    def git(self,*args):
        return subprocess.check_output(['git',*args],env=self.env,stderr=subprocess.PIPE,text=True).strip()

    def commit(self,folder,message):
        self.git('-C',str(folder),'-c','user.name=Fixture','-c','user.email=fixture@example.test','commit','-qm',message)

    def archives(self):
        return [json.loads(p.read_text()) for p in (self.root/'state/archives').glob('*.json')]

    def program_capture(self, name):
        record=self.binding(name);owner=record['worktree_owner']
        machine=json.loads(self.hgs('dirs','--creation',str(self.project)))['computer_id']
        return {'target':{'computer':'','name':name,'run_id':record['run_id'],'conversation_id':record['conversation_id']},
                'machine':machine,'root':owner['path'],'common_dir':owner['common_dir'],'owner_id':owner['id']}

    def program_manifest(self, capture, cwd=None):
        return subprocess.run([str(test_pause.HGS),'worktrees','programs','--json'],input=json.dumps({'capture':capture}),
                              env=self.env,text=True,capture_output=True,timeout=10,cwd=cwd)

    def test_program_manifest_is_read_only_and_finds_native_cwd_and_open_files(self):
        name=self.start('claude');capture=self.program_capture(name)
        # exec completion establishes the actual cwd/FDs; no readiness sleep.
        cwd=subprocess.Popen(['/bin/sleep','300'],cwd=self.project,env=self.env,stdin=subprocess.DEVNULL)
        held=open(self.project/'tracked','rb')
        opened=subprocess.Popen(['/bin/sleep','300'],cwd=self.root,env=self.env,stdin=held)
        held.close()
        self.addCleanup(lambda:self.settle_program(cwd));self.addCleanup(lambda:self.settle_program(opened))
        before=sorted(p.relative_to(self.project).as_posix() for p in self.project.rglob('*'))
        result=self.program_manifest(capture);self.assertEqual(result.returncode,0,result.stderr)
        value=json.loads(result.stdout);self.assertEqual(value['abi'],'worktree-programs-v1')
        self.assertEqual(value['capture'],capture);self.assertEqual(value['expires']-value['issued'],300)
        pids=[p['pid'] for p in value['programs']];self.assertEqual(pids,sorted(set(pids)))
        self.assertIn(cwd.pid,pids);self.assertIn(opened.pid,pids)
        self.assertIsNone(cwd.poll());self.assertIsNone(opened.poll())
        self.assertEqual(before,sorted(p.relative_to(self.project).as_posix() for p in self.project.rglob('*')))
        self.assertEqual(self.archives(),[]);self.assertTrue(self.binding(name))

    @staticmethod
    def settle_program(process):
        if process.poll() is None:process.kill()
        process.wait(timeout=5)

    def test_program_manifest_refuses_changed_original_and_replaced_ownership(self):
        name=self.start('claude');capture=self.program_capture(name)
        changed=dict(capture);changed['owner_id']='22222222-2222-4222-8222-222222222222'
        self.assertNotEqual(self.program_manifest(changed).returncode,0)
        changed=dict(capture,target=dict(capture['target'],run_id='33333333-3333-4333-8333-333333333333'))
        self.assertNotEqual(self.program_manifest(changed).returncode,0)
        marker=Path(self.git('-C',str(self.project),'rev-parse','--absolute-git-dir'))/'zerus-owner.json'
        marker.write_text('{replaced')
        self.assertNotEqual(self.program_manifest(capture).returncode,0)
        self.assertEqual(marker.read_text(),'{replaced');self.assertTrue(self.project.exists());self.assertEqual(self.archives(),[])

    def test_program_manifest_never_publishes_an_offer_from_inside_its_own_target(self):
        name=self.start('claude');capture=self.program_capture(name)
        result=self.program_manifest(capture,cwd=self.project)
        self.assertNotEqual(result.returncode,0);self.assertIn('needed to settle',result.stderr)
        self.assertTrue(self.project.exists());self.assertEqual(self.archives(),[]);self.assertTrue(self.binding(name))

    def test_program_holder_keeps_the_original_checkout_without_mutating_it_and_closes(self):
        name=self.start('claude');capture=self.program_capture(name);pipeline=str(uuid.uuid4())
        before=sorted(p.relative_to(self.project).as_posix() for p in self.project.rglob('*'))
        child=subprocess.Popen([str(test_pause.HGS),'worktrees','program-holder','--json'],
                               env=self.env,stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=subprocess.PIPE)
        try:
            child.stdin.write((json.dumps({'op':'open','pipeline':pipeline,'discovery':{'capture':capture}})+'\n').encode())
            child.stdin.flush()
            # Read bounded native frames without relying on the live holder's EOF.
            def read_frame():
                deadline=time.monotonic()+10;frame=bytearray();cap=128*1024+4096
                while not frame.endswith(b'\n') and len(frame)<=cap:
                    remaining=deadline-time.monotonic()
                    self.assertGreater(remaining,0,'the original holder did not publish its frame')
                    self.assertTrue(select.select([child.stdout],[],[],remaining)[0],'holder readiness expired')
                    chunk=os.read(child.stdout.fileno(),1)
                    if not chunk:
                        _,error=child.communicate(timeout=5)
                        self.fail('holder exited before its frame: '+error.decode(errors='replace'))
                    frame.extend(chunk)
                self.assertLessEqual(len(frame),cap)
                return json.loads(frame)
            ready=read_frame();self.assertEqual(ready['op'],'ready');self.assertEqual(ready['pipeline'],pipeline)
            self.assertEqual(ready['manifest']['capture'],capture)
            self.assertEqual(ready['holder']['pid'],child.pid);self.assertEqual(ready['holder']['parent'],os.getpid())
            self.assertTrue(Path(ready['socket']).is_socket());self.assertIsNone(child.poll())
            self.assertEqual(before,sorted(p.relative_to(self.project).as_posix() for p in self.project.rglob('*')))
            self.assertTrue(self.binding(name));self.assertEqual(self.archives(),[])
            observation=str(uuid.uuid4())
            child.stdin.write((json.dumps({'op':'observe','pipeline':pipeline,'observation':observation})+'\n').encode())
            child.stdin.flush();observed=read_frame()
            self.assertEqual(observed['op'],'observed');self.assertEqual(observed['observation'],observation)
            self.assertEqual(observed['pipeline'],pipeline)
            for field in ('capture','configuration','boot','checkout','issued','expires'):
                self.assertEqual(observed['manifest'][field],ready['manifest'][field],field)
            self.assertNotEqual(observed['manifest']['id'],ready['manifest']['id'])
            self.assertTrue(self.binding(name));self.assertEqual(self.archives(),[])
            child.stdin.write((json.dumps({'op':'close','pipeline':pipeline})+'\n').encode());child.stdin.flush()
            stdout,stderr=child.communicate(timeout=10)
            self.assertEqual(child.returncode,0,stderr.decode(errors='replace'))
            self.assertEqual(json.loads(stdout),{'op':'closed','pipeline':pipeline})
            self.assertFalse(Path(ready['socket']).exists())
            self.assertTrue(self.project.is_dir());self.assertTrue(self.binding(name));self.assertEqual(self.archives(),[])
        finally:
            self.settle_program(child)
            for stream in (child.stdin,child.stdout,child.stderr):
                if stream:stream.close()

    def command(self,*args):
        result=subprocess.run([str(test_pause.HGS),*args],env=self.env,text=True,capture_output=True,timeout=40)
        self.assertEqual(result.returncode,0,result.stdout+result.stderr)
        outcomes=[json.loads(line.split(' ',1)[1]) for line in result.stderr.splitlines() if line.startswith('ZERUS_WORKTREE_CLEANUP ')]
        self.assertEqual(len(outcomes),1,result.stderr)
        return outcomes[0]

    def test_last_terminate_removes_managed_integrated_tree_and_explicit_restore_keeps_identity(self):
        name=self.start('claude');identity=self.binding(name)['conversation_id']
        result=self.command('terminate',name);self.assertEqual(result['status'],'removed',result);self.assertFalse(self.project.exists())
        archive=self.archives()[0];self.assertEqual(archive['conversation_id'],identity)
        self.assertTrue(self.git('-C',str(self.repo),'show-ref','refs/heads/session-work'))
        self.hgs('resume',name,'--archive',archive['archive_id'],'--dry-run');self.assertFalse(self.project.exists())
        self.hgs('resume',name,'--archive',archive['archive_id'],'-d')
        self.wait(lambda:self.binding(name).get('conversation_id')==identity and not self.binding(name).get('expected_id'))
        self.assertTrue(self.project.is_dir());self.assertEqual((self.project/'tracked').read_text(),'original')

    def test_manual_archive_runs_cleanup_but_pause_alone_does_not(self):
        name=self.start('codex');self.hgs('pause',name);self.assertTrue(self.project.exists())
        result=self.command('archive',name);self.assertEqual(result['status'],'removed',result);self.assertFalse(self.project.exists())

    def test_other_session_keeps_folder_until_last_termination(self):
        first=self.start('claude','first');second=self.start('claude','second')
        result=self.command('terminate',first);self.assertEqual(result['status'],'kept',result);self.assertTrue(self.project.exists());self.assertTrue(self.binding(second))
        self.assertTrue(result['archive']['launch_owned'])
        self.assertIn(second,result['review']['sessions'])
        self.assertEqual(result['review']['path'],result['path'])
        result=self.command('terminate',second);self.assertEqual(result['status'],'removed',result)
        self.assertEqual(len(self.archives()),2)

    def test_unmerged_dirty_and_ignored_work_are_never_automatically_removed(self):
        for kind in ('unmerged','dirty','ignored'):
            with self.subTest(kind=kind):
                if kind=='unmerged':
                    (self.project/'extra').write_text('work');self.git('-C',str(self.project),'add','extra');self.commit(self.project,'not integrated')
                elif kind=='dirty':(self.project/'tracked').write_text('unsaved')
                else:
                    self.git('-C',str(self.project),'reset','--hard','main')
                    (self.repo/'.git/info/exclude').write_text('local-notes\n');(self.project/'local-notes').write_text('private data')
                name=self.start('claude',kind);result=self.command('terminate',name)
                self.assertEqual(result['status'],'kept',result);self.assertTrue(self.project.exists())

    def test_missing_nonce_or_recreated_checkout_does_not_count_as_managed(self):
        metadata=Path(self.git('-C',str(self.project),'rev-parse','--absolute-git-dir'))
        (metadata/'zerus-owner.json').unlink()
        name=self.start('claude');result=self.command('terminate',name)
        self.assertEqual(result['status'],'kept',result);self.assertIn('verified Zerus-created',result['reason']);self.assertTrue(self.project.exists())
        archive=self.archives()[0]
        self.assertEqual(result['archive']['version'],1)
        self.assertEqual(result['archive']['archive_id'],archive['archive_id'])
        self.assertEqual(result['archive']['name'],name)
        self.assertEqual(result['archive']['run_id'],archive['run_id'])
        self.assertEqual(result['archive']['conversation_id'],archive['conversation_id'])
        self.assertFalse(result['archive']['launch_owned'])
        self.assertNotIn('review',result, 'the ownership precondition must not start removal or review')

    def test_recreated_same_path_and_branch_cannot_reuse_old_ownership(self):
        self.git('-C',str(self.repo),'worktree','remove',str(self.project));self.git('-C',str(self.repo),'worktree','add','-q',str(self.project),'session-work')
        name=self.start('claude');result=self.command('terminate',name);self.assertEqual(result['status'],'kept',result);self.assertTrue(self.project.exists())

    def test_same_tree_integration_can_clean_without_deleting_the_original_branch(self):
        (self.project/'same').write_text('integrated');self.git('-C',str(self.project),'add','same');self.commit(self.project,'feature version')
        head=self.git('-C',str(self.project),'rev-parse','HEAD');(self.repo/'same').write_text('integrated');self.git('-C',str(self.repo),'add','same');self.commit(self.repo,'squashed version')
        name=self.start('claude');result=self.command('terminate',name);self.assertEqual(result['status'],'removed',result)
        self.assertEqual(self.git('-C',str(self.repo),'rev-parse','session-work'),head)

    def test_restore_refuses_changed_branch_without_creating_folder_or_starting_agent(self):
        name=self.start('claude');result=self.command('terminate',name);self.assertEqual(result['status'],'removed',result)
        archive=self.archives()[0];self.git('-C',str(self.repo),'-c','user.name=Fixture','-c','user.email=fixture@example.test','commit','--allow-empty','-qm','new main')
        self.git('-C',str(self.repo),'branch','-f','session-work','main')
        self.hgs('resume',name,'--archive',archive['archive_id'],'-d',rc=1)
        self.assertFalse(self.project.exists());self.assertEqual(len(self.archives()),1);self.assertFalse(self.binding(name))

    def test_confirmed_exit_cleanup_removes_generated_cache_and_restores_the_archive(self):
        (self.project/'Cargo.toml').write_text('[package]\nname="fixture"\nversion="0.1.0"\n')
        self.git('-C',str(self.project),'add','Cargo.toml');self.commit(self.project,'fixture manifest')
        self.git('-C',str(self.repo),'merge','--ff-only','session-work')
        (self.repo/'.git/info/exclude').write_text('target/\n')
        cache=self.project/'target';cache.mkdir()
        (cache/'CACHEDIR.TAG').write_text('Signature: 8a477f597d28d172789f06886806bc55\n')
        (cache/'generated-output').write_text('rebuildable')
        name=self.start('claude');identity=self.binding(name)['conversation_id']
        owner=self.binding(name)['worktree_owner']['id']
        listed=json.loads(self.hgs('ls','--json','--local','--no-archives'))['sessions']
        self.assertEqual(next(r for r in listed if r['name']==name)['worktree_owner_id'],owner)
        result=self.command('terminate',name);self.assertEqual(result['status'],'kept',result)
        row=json.loads(self.hgs('worktrees','review','--path',str(self.repo),'--worktree',str(self.project),'--json'))['worktrees'][0]
        self.assertTrue(row['zerus_owned']);self.assertTrue(row['archive_restore_ready']);self.assertTrue(row['generated_files_only'])
        head=self.git('-C',str(self.project),'rev-parse','HEAD')
        self.hgs('worktrees','remove','--path',str(self.project),'--common-dir',str(self.repo/'.git'),
                 '--fingerprint',row['fingerprint'],'--ignored-fingerprint',row['ignored_fingerprint'],'--preserve-archive-restore','--json')
        self.assertFalse(self.project.exists());self.assertEqual(self.git('-C',str(self.repo),'rev-parse','session-work'),head)
        archive=self.archives()[0];self.assertEqual(archive['worktree_cleanup']['head'],head)
        self.hgs('resume',name,'--archive',archive['archive_id'],'-d')
        self.wait(lambda:self.binding(name).get('conversation_id')==identity and not self.binding(name).get('expected_id'))
        self.assertTrue(self.project.is_dir());self.assertFalse((self.project/'target').exists())

    def test_new_managed_checkout_at_the_same_path_invalidates_the_old_review(self):
        def review():return json.loads(self.hgs('worktrees','review','--path',str(self.repo),'--worktree',str(self.project),'--json'))['worktrees'][0]
        original=review();head=self.git('-C',str(self.project),'rev-parse','HEAD')
        self.git('-C',str(self.repo),'worktree','remove',str(self.project))
        self.git('-C',str(self.repo),'branch','-D','session-work')
        self.hgs('worktrees','create','--path',str(self.repo),'--destination',str(self.project),'--branch','session-work','--json')
        replacement=review()
        self.assertEqual(replacement['head'],head);self.assertEqual(replacement['branch'],original['branch'])
        self.assertNotEqual(replacement['ownership_id'],original['ownership_id'])
        self.assertNotEqual(replacement['fingerprint'],original['fingerprint'])
        self.hgs('worktrees','remove','--path',str(self.project),'--common-dir',str(self.repo/'.git'),
                 '--fingerprint',original['fingerprint'],'--json',rc=1)
        self.assertTrue(self.project.is_dir())

    def test_termination_keeps_a_new_managed_checkout_at_the_original_path(self):
        name=self.start('claude');owner=self.binding(name)['worktree_owner']['id']
        self.git('-C',str(self.repo),'worktree','remove','--force',str(self.project))
        self.hgs('worktrees','create','--path',str(self.repo),'--destination',str(self.project),'--branch','replacement-work','--json')
        row=json.loads(self.hgs('worktrees','review','--path',str(self.repo),'--worktree',str(self.project),'--json'))['worktrees'][0]
        self.assertNotEqual(row['ownership_id'],owner)
        result=self.command('terminate',name)
        self.assertEqual(result['status'],'kept',result);self.assertIn('ownership changed',result['reason'])
        self.assertTrue(self.project.is_dir());self.assertNotIn('worktree_cleanup',self.archives()[0])

    def test_unknown_local_data_and_stale_cache_review_do_not_allow_exit_cleanup(self):
        (self.repo/'.git/info/exclude').write_text('local-notes\n')
        note=self.project/'local-notes';note.write_text('retain unique data')
        name=self.start('claude');self.command('terminate',name)
        row=json.loads(self.hgs('worktrees','review','--path',str(self.repo),'--worktree',str(self.project),'--json'))['worktrees'][0]
        self.assertTrue(row['zerus_owned']);self.assertFalse(row['generated_files_only'])
        note.write_text('changed after the review')
        self.hgs('worktrees','remove','--path',str(self.project),'--common-dir',str(self.repo/'.git'),
                 '--fingerprint',row['fingerprint'],'--ignored-fingerprint',row['ignored_fingerprint'],'--preserve-archive-restore','--json',rc=1)
        self.assertEqual(note.read_text(),'changed after the review')
        self.assertNotIn('worktree_cleanup',self.archives()[0])

    def test_restore_never_follows_a_replaced_destination_parent(self):
        parent=self.root/'parent';parent.mkdir();self.project=parent/'linked'
        self.hgs('worktrees','create','--path',str(self.repo),'--destination',str(self.project),'--branch','nested-session','--json')
        (self.root/'cfg/projects.local').write_text('p='+str(self.project)+'\n')
        name=self.start('claude');result=self.command('terminate',name);self.assertEqual(result['status'],'removed',result);archive=self.archives()[0]
        parent.rename(self.root/'original-parent');other=self.root/'other';other.mkdir();parent.symlink_to(other,target_is_directory=True)
        self.hgs('resume',name,'--archive',archive['archive_id'],'-d',rc=1)
        self.assertFalse((other/'linked').exists());self.assertFalse(self.binding(name))


if __name__=='__main__':unittest.main()
