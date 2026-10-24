"""Process-inventory fixtures retain complete coverage without unrelated native exec races."""
import contextlib
import io
import os
import tempfile
import importlib.util
from pathlib import Path
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location('integration_driver', ROOT/'scripts/ci/python-tests.py')
driver = importlib.util.module_from_spec(spec)
spec.loader.exec_module(driver)


class IntegrationScheduling(unittest.TestCase):
    def test_mac_process_scope_keeps_all_three_whole_modules(self):
        with patch.object(driver.sys, 'platform', 'darwin'), patch.object(driver.subprocess, 'run') as run:
            actual = [item for module in sorted(driver.WORKTREE_MODULES) for item in driver.jobs_for(module)]
        self.assertEqual(actual, [(name, [name]) for name in
            ['test_worktree_lifecycle', 'test_worktree_maintenance', 'test_worktrees']])
        run.assert_not_called()

    def test_only_the_mac_process_scope_waits_and_no_jobs_disappear(self):
        jobs = [(name, [name]) for name in ['test_accounts.Accounts', 'test_worktrees',
            'test_worktree_lifecycle', 'test_questions.Questions', 'test_worktree_maintenance']]
        parallel, deferred = driver.partition_jobs(jobs, 'darwin')
        self.assertEqual([name for name, _ in parallel], ['test_accounts.Accounts', 'test_questions.Questions'])
        self.assertEqual([name for name, _ in deferred],
            ['test_worktrees', 'test_worktree_lifecycle', 'test_worktree_maintenance'])
        self.assertEqual(sorted(parallel + deferred), sorted(jobs))
        for system in ['linux', 'win32']:
            self.assertEqual(driver.partition_jobs(jobs, system), (jobs, []))

    def test_real_subprocess_jobs_finish_before_the_mac_inventory_scope(self):
        def execute(unsequenced):
            with tempfile.TemporaryDirectory(prefix='zerus-driver-order-') as folder:
                root=Path(folder); tests=root/'tests';tests.mkdir();results=root/'results'
                ordinary = """import unittest,time
from pathlib import Path
ROOT=Path(__file__).parent
class Fast(unittest.TestCase):
 def test_finish(self):time.sleep(.01);(ROOT/'fast.done').write_text('done')
class Slow(unittest.TestCase):
 def test_finish(self):time.sleep(.3);(ROOT/'slow.done').write_text('done')
""" + ('# unrelated ordinary fixture metadata\n' * 30)
                (tests/'test_ordinary.py').write_text(ordinary)
                for name in driver.WORKTREE_MODULES:
                    (tests/(name+'.py')).write_text("""import unittest
from pathlib import Path
class Scope(unittest.TestCase):
 def test_all_unrelated_jobs_finished(self):
  root=Path(__file__).parent
  self.assertTrue((root/'fast.done').is_file())
  self.assertTrue((root/'slow.done').is_file())
""")
                partition=(lambda jobs:(list(jobs),[])) if unsequenced else driver.partition_jobs
                with patch.multiple(driver, ROOT=root, TESTS=tests, RESULTS=results,
                        DURATIONS=results/'durations.json', SEED=root/'absent-seed.json', partition_jobs=partition), \
                     patch.object(driver.sys,'platform','darwin'), \
                     patch.dict(os.environ,{'ZERUS_PYTHON_JOBS':'2'}), contextlib.redirect_stdout(io.StringIO()):
                    failed=driver.main()
                self.assertEqual(failed,unsequenced)
                for name in driver.WORKTREE_MODULES:
                    self.assertIn('test_all_unrelated_jobs_finished',(results/(name+'.log')).read_text())
        execute(True)  # The original concurrent scheduling actually violates the prerequisite.
        execute(False)


if __name__ == '__main__':
    unittest.main()
