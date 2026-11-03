"""Only synthetic owned descendants; no repository worker is started here."""
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


if __name__ == '__main__':
    unittest.main()
