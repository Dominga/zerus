"""Only synthetic owned descendants; no repository worker is started here."""
import errno
import importlib.util
import os
from pathlib import Path
import subprocess
import sys
import signal
import unittest

RUNNER = Path(__file__).with_name('bounded_process.py')


@unittest.skipUnless(sys.platform.startswith('linux'), 'Linux pidfd fixture cleanup')
class BoundedProcess(unittest.TestCase):
    def run_fixture(self, code, seconds=2, limit=8):
        return subprocess.run([sys.executable, str(RUNNER), '--seconds', str(seconds),
                               '--max-processes', str(limit), '--', sys.executable, '-c', code],
                              capture_output=True, text=True, timeout=8)

    def test_root_exit_reaps_a_new_session_grandchild(self):
        result = self.run_fixture("import subprocess,sys; p=subprocess.Popen([sys.executable,'-c','import time;time.sleep(30)'],start_new_session=True); print(p.pid,flush=True)")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertFalse(Path('/proc', result.stdout.strip()).exists())

    def test_adopted_exit_is_reaped_while_the_original_keeps_its_failure_status(self):
        # The original test stays alive until its orphan has disappeared, as
        # stream::helper's group checks do. Its nonzero result must survive:
        # reaping all children would steal Popen's status and mask the failure.
        orphan = """import os,sys,time
from pathlib import Path
gate=Path(sys.argv[1])
while not gate.exists(): time.sleep(.005)
"""
        parent = """import subprocess,sys
child=subprocess.Popen([sys.executable,'-c',sys.argv[1],sys.argv[2]],
                       stdin=subprocess.DEVNULL,stdout=subprocess.DEVNULL,
                       stderr=subprocess.DEVNULL)
print(child.pid,flush=True)
"""
        code = f'''import subprocess,sys,tempfile,time
from pathlib import Path
with tempfile.TemporaryDirectory() as directory:
 gate=Path(directory)/'release'
 parent=subprocess.run([sys.executable,'-c',{parent!r},{orphan!r},str(gate)],
                       capture_output=True,text=True,check=True)
 pid=int(parent.stdout)
 gate.touch()
 deadline=time.monotonic()+2
 while Path('/proc',str(pid)).exists():
  assert time.monotonic()<deadline,'adopted zombie was not reaped during the test'
  time.sleep(.005)
 print('orphan reaped before original exit',flush=True)
 sys.exit(17)
'''
        result = self.run_fixture(code, seconds=4)
        self.assertEqual(result.returncode, 17, result.stderr)
        self.assertEqual(result.stdout.strip(), 'orphan reaped before original exit')

    def test_deadline_cleans_owned_tree_and_keeps_unrelated_child(self):
        other = subprocess.Popen([sys.executable, '-c', 'import time;time.sleep(30)'])
        try:
            result = self.run_fixture("import subprocess,sys,time; p=subprocess.Popen([sys.executable,'-c','import time;time.sleep(30)'],start_new_session=True); print(p.pid,flush=True); time.sleep(30)", seconds=.3)
            self.assertEqual(result.returncode, 124, result.stderr)
            self.assertFalse(Path('/proc', result.stdout.strip()).exists())
            self.assertIsNone(other.poll())
        finally:
            other.terminate()
            other.wait(timeout=3)

    def test_process_budget_stops_only_its_original_tree(self):
        result = self.run_fixture("import subprocess,sys,time; children=[subprocess.Popen([sys.executable,'-c','import time;time.sleep(30)']) for _ in range(8)]; time.sleep(30)", limit=3)
        self.assertEqual(result.returncode, 125, result.stderr)

    def test_cancelled_supervisor_reaps_its_new_session_descendant(self):
        command = [sys.executable, str(RUNNER), '--seconds', '2', '--', sys.executable,
                   '-c', "import subprocess,sys,time; p=subprocess.Popen([sys.executable,'-c','import time;time.sleep(30)'],start_new_session=True); print(p.pid,flush=True); time.sleep(30)"]
        for sig in (signal.SIGTERM, signal.SIGINT, signal.SIGHUP):
            with self.subTest(signal=sig):
                process = subprocess.Popen(command, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
                try:
                    # The supervisor also has its own two-second bound if publication fails.
                    descendant = process.stdout.readline().strip()
                    process.send_signal(sig)
                    _, error = process.communicate(timeout=6)
                    self.assertEqual(process.returncode, 128 + sig, error)
                    self.assertFalse(Path('/proc', descendant).exists())
                finally:
                    if process.poll() is None:
                        process.terminate()
                        process.communicate(timeout=6)

    def test_transient_signal_denials_retain_custody_until_cleanup_can_finish(self):
        # No real privilege transition: deny each retained-fd operation once
        # inside a second owned supervisor, with an outer cleanup bound too.
        code = f'''import importlib.util,sys,signal
spec=importlib.util.spec_from_file_location('bounded', {str(RUNNER)!r})
b=importlib.util.module_from_spec(spec);spec.loader.exec_module(b)
original=b.signal.pidfd_send_signal
remaining={{0:1,signal.SIGSTOP:1,signal.SIGKILL:1}}
def transient(fd,sig,*args):
 if remaining.get(sig,0):
  remaining[sig]-=1
  raise PermissionError(1,'synthetic transient signal denial')
 return original(fd,sig,*args)
b.signal.pidfd_send_signal=transient
sys.exit(b.supervise([sys.executable,'-c','import os,time;print(os.getpid(),flush=True);time.sleep(30)'],.2,8))
'''
        result = self.run_fixture(code)
        self.assertEqual(result.returncode, 124, result.stderr)
        self.assertFalse(Path('/proc', result.stdout.strip()).exists())

    def test_prune_retains_original_grandchild_when_parent_cannot_be_revisited(self):
        # A later collect must not mask a lost prune entry by re-adopting it.
        # Both generations are real children inside two bounded supervisors.
        parent = "import subprocess,sys,time; child=subprocess.Popen([sys.executable,'-c','import time;time.sleep(30)']); print(child.pid,flush=True); time.sleep(30)"
        code = f'''import importlib.util,sys,signal,subprocess
spec=importlib.util.spec_from_file_location('bounded', {str(RUNNER)!r})
b=importlib.util.module_from_spec(spec);spec.loader.exec_module(b)
assert b.ctypes.CDLL(None,use_errno=True).prctl(36,1,0,0,0)==0
parent=subprocess.Popen([sys.executable,'-c',{parent!r}],stdout=subprocess.PIPE,text=True)
tree=b.Tree(8)
original=b.signal.pidfd_send_signal
try:
 grandchild=int(parent.stdout.readline())
 tree.collect()
 retained=tree.owned[grandchild]
 assert parent.pid in tree.owned and parent.poll() is None
 phase='prune'
 denials={{'prune':0,'collect':0}}
 def transient(fd,sig,*args):
  denied=(phase=='prune' and fd==retained[1]) or (phase=='collect' and fd==tree.owned[parent.pid][1])
  if sig==0 and denied:
   denials[phase]+=1
   raise PermissionError(1,'synthetic transient custody denial')
  return original(fd,sig,*args)
 b.signal.pidfd_send_signal=transient
 tree.prune()
 phase='collect'
 tree.collect()
 assert denials=={{'prune':1,'collect':1}},denials
 assert tree.owned.get(grandchild)==retained,'prune discarded the original grandchild pidfd'
 print('original grandchild custody retained',flush=True)
finally:
 b.signal.pidfd_send_signal=original
 assert tree.cleanup(),'private original descendants remain'
 tree.close()
 parent.stdout.close()
 parent.returncode=-signal.SIGKILL
'''
        result = self.run_fixture(code)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout.strip(), 'original grandchild custody retained')

    def test_cleanup_cannot_report_clean_when_proc_hides_a_live_child(self):
        # Hide discovery, not the kernel child. Restore discovery in finally so
        # even the old false-clean implementation leaves no fixture behind.
        code = f'''import importlib.util,sys,signal,subprocess
spec=importlib.util.spec_from_file_location('bounded', {str(RUNNER)!r})
b=importlib.util.module_from_spec(spec);spec.loader.exec_module(b)
assert b.ctypes.CDLL(None,use_errno=True).prctl(36,1,0,0,0)==0
child=subprocess.Popen([sys.executable,'-c','import time;time.sleep(30)'])
tree=b.Tree(8)
original=b.children
try:
 b.children=lambda pid:set()
 assert not tree.cleanup(),'unreadable proc children falsely proved clean'
 assert child.poll() is None
 print('kernel child prevents clean',flush=True)
finally:
 b.children=original
 assert tree.cleanup(),'private original child remains'
 tree.close()
 child.returncode=-signal.SIGKILL
'''
        result = self.run_fixture(code, seconds=5)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout.strip(), 'kernel child prevents clean')

    def test_unconfirmed_cleanup_fails_with_original_identities_and_outer_owner_reaps(self):
        code = f'''import importlib.util,sys,signal
spec=importlib.util.spec_from_file_location('bounded', {str(RUNNER)!r})
b=importlib.util.module_from_spec(spec);spec.loader.exec_module(b)
original=b.signal.pidfd_send_signal
def denied(fd,sig,*args):
 if sig==signal.SIGKILL: raise PermissionError(1,'synthetic retained kill denial')
 return original(fd,sig,*args)
b.signal.pidfd_send_signal=denied
sys.exit(b.supervise([sys.executable,'-c','import os,time;print(os.getpid(),flush=True);time.sleep(30)'],.4,8))
'''
        result = self.run_fixture(code, seconds=5)
        self.assertEqual(result.returncode, 126, result.stderr)
        self.assertIn('original identities', result.stderr)
        self.assertIn('original descendants remain', result.stderr)
        self.assertFalse(Path('/proc', result.stdout.strip()).exists())


def load():
    """The supervisor module itself, for checks that start no process."""
    spec = importlib.util.spec_from_file_location('bounded_scan', RUNNER)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


class ScriptedProc:
    """A /proc whose reads follow a script: a path maps to its text, the
    names it lists, or the error reading it raises. A listing may raise
    part-way through, as a scandir of an exiting task can."""

    def __init__(self, script, *parts):
        self.script, self.parts = script, parts

    def __truediv__(self, part):
        return ScriptedProc(self.script, *self.parts, str(part))

    def entry(self):
        entry = self.script[self.parts]
        if isinstance(entry, BaseException):
            raise entry
        return entry

    def iterdir(self):
        for name in self.entry():
            if isinstance(name, BaseException):
                raise name
            yield self / name

    def read_text(self):
        return self.entry()


@unittest.skipUnless(sys.platform.startswith('linux'), 'Linux /proc scan')
class ProcScanRace(unittest.TestCase):
    """zerus-abyw: a process that exits while /proc is scanned ends the scan
    of that process, never the supervisor."""

    def scan(self, script):
        bounded = load()
        bounded.Path = lambda root: ScriptedProc(script, root)
        return bounded.children(10)

    def test_a_process_that_exits_before_its_tasks_are_read_lists_nothing(self):
        for error in (ProcessLookupError(errno.ESRCH, 'No such process'),
                      FileNotFoundError(errno.ENOENT, 'No such file or directory')):
            with self.subTest(error=error):
                self.assertEqual(self.scan({('/proc', '10', 'task'): error}), set())

    def test_a_thread_that_exits_mid_scan_keeps_what_the_others_listed(self):
        esrch = ProcessLookupError(errno.ESRCH, 'No such process')
        threads = {('/proc', '10', 'task', '10', 'children'): '20 21',
                   ('/proc', '10', 'task', '11', 'children'): esrch,
                   ('/proc', '10', 'task', '12', 'children'): '22'}
        self.assertEqual(self.scan({('/proc', '10', 'task'): ['10', '11', '12'], **threads}), {20, 21, 22})
        # The listing itself fails after its first thread.
        partial = {('/proc', '10', 'task'): ['10', esrch], **threads}
        self.assertEqual(self.scan(partial), {20, 21})

    def test_an_unreadable_process_lists_nothing_and_other_errors_still_fail(self):
        denied = PermissionError(errno.EACCES, 'Permission denied')
        self.assertEqual(self.scan({('/proc', '10', 'task'): denied}), set())
        with self.assertRaises(OSError):
            self.scan({('/proc', '10', 'task'): OSError(errno.EIO, 'Input/output error')})

    def test_a_collect_racing_an_exiting_child_keeps_it_in_custody(self):
        # A real owned child inside a bounded supervisor; its task directory
        # reads as exiting (ESRCH), then as unreadable (EACCES). Neither ends
        # the collect, and neither drops the child from custody.
        code = f"""import errno,importlib.util,sys,signal,subprocess
spec=importlib.util.spec_from_file_location('bounded', {str(RUNNER)!r})
b=importlib.util.module_from_spec(spec);spec.loader.exec_module(b)
assert b.ctypes.CDLL(None,use_errno=True).prctl(36,1,0,0,0)==0
child=subprocess.Popen([sys.executable,'-c','import time;time.sleep(30)'])
tree=b.Tree(8)
real=b.Path
fault=[]
class Racing:
 def __init__(self,*parts): self.path=real(*parts)
 def __truediv__(self,part): return Racing(self.path/part)
 def iterdir(self):
  if fault and self.path==real('/proc',str(child.pid),'task'): raise fault[0]
  return (Racing(path) for path in self.path.iterdir())
 def read_text(self): return self.path.read_text()
try:
 tree.collect()
 started=tree.owned[child.pid][0]
 b.Path=Racing
 for error in (ProcessLookupError(errno.ESRCH,'No such process'),PermissionError(errno.EACCES,'Permission denied')):
  fault[:]=[error]
  tree.collect()
  tree.prune()
  assert tree.owned.get(child.pid,('gone',))[0]==started,error
 print('custody kept through the race',flush=True)
finally:
 b.Path=real
 assert tree.cleanup(),'private original child remains'
 tree.close()
 child.returncode=-signal.SIGKILL
"""
        result = subprocess.run([sys.executable, str(RUNNER), '--seconds', '5', '--max-processes', '8',
                                 '--', sys.executable, '-c', code],
                                capture_output=True, text=True, timeout=12)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout.strip(), 'custody kept through the race')


if __name__ == '__main__':
    unittest.main()
