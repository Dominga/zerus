"""Process-inventory fixtures retain complete coverage without unrelated native exec races."""
import contextlib
import io
import os
import subprocess
import tempfile
import importlib.util
from pathlib import Path
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location('integration_driver', ROOT/'scripts/ci/python-tests.py')
driver = importlib.util.module_from_spec(spec)
spec.loader.exec_module(driver)
scope_spec = importlib.util.spec_from_file_location('mac_worktree_scope', ROOT/'scripts/ci/macos-worktree-tests.py')
scope = importlib.util.module_from_spec(scope_spec)
scope_spec.loader.exec_module(scope)


class HostedMacAdmission(unittest.TestCase):
    def test_owner_and_non_upstream_hosts_refuse_before_allocation_or_sudo(self):
        admitted = dict(GITHUB_ACTIONS='true', RUNNER_ENVIRONMENT='github-hosted', GITHUB_REPOSITORY='ufna/zerus')
        for system, changes in [('Darwin', {'GITHUB_ACTIONS': ''}),
                ('Darwin', {'RUNNER_ENVIRONMENT': 'self-hosted'}),
                ('Darwin', {'GITHUB_REPOSITORY': 'fixture/fork'}), ('Linux', {})]:
            with self.subTest(system=system, changes=changes), \
                    patch.dict(os.environ, dict(admitted, **changes), clear=True), \
                    patch.object(scope.platform, 'system', return_value=system), \
                    patch.object(scope, 'command') as command, \
                    patch.object(scope.tempfile, 'mkdtemp') as allocate:
                with self.assertRaisesRegex(ValueError, 'disposable hosted runner'):
                    scope.run('test_worktrees', Path('/nonexistent/normal-hgs'))
                command.assert_not_called()
                allocate.assert_not_called()

    def test_unknown_directory_service_failure_never_proves_account_absence(self):
        for code, output in [(1, 'permission denied'), (56, 'unrelated error')]:
            with self.subTest(code=code), patch.object(scope, 'dscl',
                    return_value=subprocess.CompletedProcess([], code, output, '')):
                with self.assertRaisesRegex(ValueError, 'could not be verified'):
                    scope.identity('_fixture')
        with patch.object(scope, 'dscl', return_value=subprocess.CompletedProcess(
                [], 56, '<dscl_cmd> DS Error: -14136 (eDSRecordNotFound)', '')):
            self.assertIsNone(scope.identity('_fixture'))

    def test_changed_scope_identity_never_authorizes_process_signals(self):
        with patch.object(scope, 'identity', return_value={'GeneratedUID': 'different'}), \
                patch.object(scope, 'command') as command:
            with self.assertRaisesRegex(ValueError, 'no signal is authorized'):
                scope.signal_scope('_fixture', 60000, {'GeneratedUID': 'original'}, 'TERM')
            command.assert_not_called()

    def test_unknown_launchd_error_never_proves_domain_absence(self):
        with patch.object(scope, 'privileged', return_value=subprocess.CompletedProcess(
                [], 1, '', 'Could not print domain: 1: Operation not permitted')):
            with self.assertRaisesRegex(ValueError, 'could not be verified'):
                scope.user_domain(60000)
        with patch.object(scope, 'privileged', return_value=subprocess.CompletedProcess(
                [], 0, 'system = {\n\tsubdomains = {\n\t\tuser/501\n\t\tpid/12\n\t}\n}\n', '')):
            self.assertFalse(scope.user_domain(60000))
            self.assertTrue(scope.user_domain(501))
        with patch.object(scope, 'privileged', return_value=subprocess.CompletedProcess(
                [], 0, 'system = {\n}\n', '')):
            with self.assertRaisesRegex(ValueError, 'Unexpected launchd subdomain inventory'):
                scope.user_domain(60000)

    def test_preexisting_or_changed_domain_never_authorizes_bootout(self):
        for absent, actual in [(False, {'UniqueID': '60000'}), (True, {'UniqueID': 'different'})]:
            with self.subTest(absent=absent), patch.object(scope, 'identity', return_value=actual), \
                    patch.object(scope, 'privileged') as privileged:
                with self.assertRaisesRegex(ValueError, 'no bootout is authorized'):
                    scope.close_domain('_fixture', 60000, {'UniqueID': '60000'}, absent)
                privileged.assert_not_called()


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
                     patch.dict(os.environ,{'ZERUS_PYTHON_JOBS':'2','GITHUB_ACTIONS':'','RUNNER_ENVIRONMENT':''}), contextlib.redirect_stdout(io.StringIO()):
                    failed=driver.main()
                self.assertEqual(failed,unsequenced)
                for name in driver.WORKTREE_MODULES:
                    self.assertIn('test_all_unrelated_jobs_finished',(results/(name+'.log')).read_text())
        execute(True)  # The original concurrent scheduling actually violates the prerequisite.
        execute(False)


if __name__ == '__main__':
    unittest.main()
