#!/usr/bin/env python3
"""Unix termination through real hgs supervisors and a private tmux socket."""
import json
import os
import signal
import shlex
import shutil
import subprocess
import sys
import unittest
import test_pause

SPAWN = r'''
children=[]
for name in ['first','second']:
 child=subprocess.Popen(['sh','-c','trap "" TERM; sleep 300 & echo $! > "$1"; wait','sh',str(home/(name+'-child'))],start_new_session=True)
 children.append(child)
(home/'children').write_text(json.dumps([p.pid for p in children]))
'''

@unittest.skipUnless(test_pause.TMUX and sys.platform in ('linux','darwin'), 'Linux/macOS and private tmux required')
class TerminateProcesses(test_pause.Harness):
    wait_for=test_pause.Harness.wait
    def setUp(self):
        self.children=[]
        super().setUp()
        marker='event("SessionStart")\nfor line in sys.stdin:'
        assert test_pause.AGENT.count(marker)==1
        source=test_pause.AGENT.replace(marker,SPAWN+'\n'+marker)
        source=source.replace('for line in sys.stdin:','for line in sys.stdin:\n'
            '    if line.strip()=="terminate-self":\n'
            '        result=subprocess.run(['+repr(str(test_pause.HGS))+',"terminate",os.environ["HGS_SESSION"]],capture_output=True,text=True)\n'
            '        (home/"self-result").write_text(json.dumps(dict(code=result.returncode,error=result.stderr)))\n'
            '        continue\n')
        self.script('claude',source)
        self.name=self.start('claude')
        self.record=self.binding(self.name)
        self.run_id=self.record['run_id'];self.conversation_id=self.record['conversation_id']
        self.state=self.root/'state'
        self.children=json.loads((self.root/'children').read_text())
        self.child_starts={pid:subprocess.check_output(['ps','-p',str(pid),'-o','lstart='],text=True).strip() for pid in self.children}
    def tearDown(self):
        # These detached jobs deliberately survive a failed termination. Reap
        # the exact fixture groups before the harness verifies its own cleanup.
        for pid in self.children:
            start=subprocess.run(['ps','-p',str(pid),'-o','lstart='],text=True,capture_output=True).stdout.strip()
            if start and start==self.child_starts[pid]:
                try:os.killpg(pid,signal.SIGKILL)
                except ProcessLookupError:pass
        super().tearDown()
    def jobs(self):
        return [j for j in json.loads(self.hgs('processes',self.name))['items'] if j['source']=='process_tree']
    def states(self,pids):
        return {pid:subprocess.run(['ps','-p',str(pid),'-o','stat='],text=True,capture_output=True,timeout=3).stdout.strip() for pid in pids}

    def tree(self):
        self.wait_for(lambda:all((self.root/(name+'-child')).exists() and (self.root/(name+'-child')).read_text().strip() for name in ('first','second')))
        return self.children+[int((self.root/(name+'-child')).read_text()) for name in ('first','second')]

    def terminate(self,*args,ok=True):
        result=subprocess.run([str(test_pause.HGS),'terminate',self.name,*args],env=self.env,text=True,capture_output=True,timeout=25)
        if ok:self.assertEqual(result.returncode,0,result.stderr)
        else:self.assertNotEqual(result.returncode,0)
        return result

    def test_terminate_reaps_detached_jobs_and_preserves_foreign_process_in_same_folder(self):
        tree=self.tree();self.assertEqual(len(self.jobs()),2)
        foreign=subprocess.Popen(['sleep','300'],cwd=self.root)
        self.addCleanup(lambda:foreign.poll() is None and (foreign.terminate(),foreign.wait(timeout=5)))
        self.terminate('--expected-run-id',self.run_id)
        self.assertTrue(all(not state or state.startswith(('Z','X')) for state in self.states(tree).values()),self.states(tree))
        self.assertIsNone(foreign.poll())
        self.assertFalse(self.states([foreign.pid])[foreign.pid].startswith('T'))
        archives=list((self.state/'archives').glob('*.json'))
        self.assertEqual(len(archives),1)
        self.assertEqual(json.loads(archives[0].read_text())['conversation_id'],self.conversation_id)

    def test_terminate_dry_run_and_stale_run_leave_the_entire_tree_running(self):
        tree=self.tree()+[self.record['pid']]
        self.terminate('--dry-run')
        self.terminate('--expected-run-id','different-run',ok=False)
        self.assertTrue(all(state and not state.startswith(('T','Z','X')) for state in self.states(tree).values()),self.states(tree))

    def test_already_suspended_agent_and_children_can_be_terminated(self):
        tree=self.tree()+[self.record['pid']]
        for pid in (self.record['pid'],self.children[0]):os.kill(pid,signal.SIGSTOP)
        self.wait_for(lambda:all(self.states([pid])[pid].startswith('T') for pid in (self.record['pid'],self.children[0])))
        self.terminate()
        states=self.states(tree)
        self.assertTrue(all(not state or state.startswith(('Z','X')) for state in states.values()),states)

    def test_self_termination_refuses_before_suspending_caller_ancestors(self):
        tree=self.tree()+[self.record['pid']]
        self.t('send-keys','-t','='+self.name+':','terminate-self','Enter')
        path=self.root/'self-result';self.wait_for(lambda:path.exists() and path.read_text().strip())
        result=json.loads(path.read_text());self.assertNotEqual(result['code'],0)
        self.assertIn('outside its own process tree',result['error'])
        states=self.states(tree)
        self.assertTrue(all(state and not state.startswith(('T','Z','X')) for state in states.values()),states)

    def test_failed_terminal_teardown_thaws_only_processes_it_suspended(self):
        tree=self.tree();os.kill(self.children[0],signal.SIGSTOP)
        self.wait_for(lambda:self.states([self.children[0]])[self.children[0]].startswith('T'))
        real_tmux=shlex.quote(test_pause.TMUX);socket=shlex.quote(str(self.socket))
        self.script('tmux','#!/bin/sh\nif [ "$1" = kill-session ]; then echo "fixture teardown refused" >&2; exit 1; fi\nexec '+real_tmux+' -S '+socket+' -f /dev/null "$@"\n')
        result=self.terminate(ok=False);self.assertIn('fixture teardown refused',result.stderr)
        states=self.states(tree+[self.record['pid']])
        self.assertTrue(all(state and not state.startswith(('Z','X')) for state in states.values()),states)
        self.assertTrue(states[self.children[0]].startswith('T'),states)
        self.assertTrue(all(not state.startswith('T') for pid,state in states.items() if pid!=self.children[0]),states)
        self.assertTrue(bool(self.binding(self.name)));self.assertFalse(list((self.state/'archives').glob('*.json')))

    def test_unavailable_process_inventory_keeps_terminal_and_jobs(self):
        tree=self.tree()+[self.record['pid']]
        self.script('ps','#!/bin/sh\nif [ "$2" = pid=,ppid=,pgid= ]; then exit 1; fi\nexec '+shlex.quote(shutil.which('ps'))+' "$@"\n')
        result=self.terminate(ok=False);self.assertIn('process tree',result.stderr)
        self.assertTrue(all(state and not state.startswith(('T','Z','X')) for state in self.states(tree).values()),self.states(tree))
        self.assertTrue(bool(self.binding(self.name)))

    def test_inventory_failure_after_suspending_agent_restores_it(self):
        tree=self.tree()+[self.record['pid']]
        real=shlex.quote(shutil.which('ps'));pid=str(self.record['pid'])
        self.script('ps','#!/bin/sh\nif [ "$2" = pid=,ppid=,pgid= ]; then\n state=$('+real+' -p '+pid+' -o stat=)\n case "$state" in *T*) exit 1;; esac\nfi\nexec '+real+' "$@"\n')
        result=self.terminate(ok=False);self.assertIn('process tree',result.stderr)
        self.assertTrue(all(state and not state.startswith(('T','Z','X')) for state in self.states(tree).values()),self.states(tree))
        self.assertTrue(bool(self.binding(self.name)))

    def test_externally_resumed_child_keeps_terminal_and_restores_temporary_stops(self):
        tree=self.tree()+[self.record['pid']]
        real=shlex.quote(shutil.which('ps'));pid=str(self.children[0])
        self.script('ps','#!/bin/sh\nif [ "$2" = pid=,ppid=,pgid= ]; then\n state=$('+real+' -p '+pid+' -o stat=)\n'
            ' case "$state" in *T*) kill -CONT '+pid+';; esac\nfi\nexec '+real+' "$@"\n')
        result=self.terminate(ok=False);self.assertIn('resumed during verification',result.stderr)
        states=self.states(tree)
        self.assertTrue(all(state and not state.startswith(('T','Z','X')) for state in states.values()),states)
        self.assertTrue(bool(self.binding(self.name)))

    def test_child_created_during_verification_is_also_stopped(self):
        # A live agent child forks when the parent has become suspended. The
        # next complete roster must capture that new child before teardown.
        script=self.root/'racing-child.py'
        script.write_text('import os,pathlib,subprocess,time\nroot=pathlib.Path(os.environ["HOME"])\n'
            'while not (root/"fork-now").exists():time.sleep(.002)\n'
            'p=subprocess.Popen(["sleep","300"])\n(root/"late-pid").write_text(str(p.pid))\np.wait()\n')
        # Ask the fixture agent itself to start the spawner, preserving real
        # ancestry instead of attaching an unrelated process to a fake record.
        source=test_pause.AGENT.replace('event("SessionStart")\nfor line in sys.stdin:',SPAWN+'\n'
            'children.append(subprocess.Popen([sys.executable,'+repr(str(script))+'],start_new_session=True))\n'
            '(home/"children").write_text(json.dumps([p.pid for p in children]))\n'
            'event("SessionStart")\nfor line in sys.stdin:')
        self.script('claude',source)
        self.terminate()
        self.name=self.start('claude','racing');self.record=self.binding(self.name);self.run_id=self.record['run_id']
        self.children=json.loads((self.root/'children').read_text())
        self.child_starts={pid:subprocess.check_output(['ps','-p',str(pid),'-o','lstart='],text=True).strip() for pid in self.children}
        real=shlex.quote(shutil.which('ps'));pid=str(self.record['pid'])
        self.script('ps','#!/bin/sh\nif [ "$2" = pid=,ppid=,pgid= ]; then\n state=$('+real+' -p '+pid+' -o stat=)\n'
            ' case "$state" in *T*) touch '+shlex.quote(str(self.root/'fork-now'))+'; while [ ! -s '+shlex.quote(str(self.root/'late-pid'))+' ]; do sleep .002; done;; esac\n'
            'fi\nexec '+real+' "$@"\n')
        tree=self.tree();self.terminate()
        late=int((self.root/'late-pid').read_text());states=self.states(tree+[late])
        self.assertTrue(all(not state or state.startswith(('Z','X')) for state in states.values()),states)


if __name__=='__main__':unittest.main()
