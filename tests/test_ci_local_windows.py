"""Windows local planning must not turn unexecuted checks green."""
import importlib.util
from pathlib import Path
import sys
import tempfile
import os
import subprocess
import unittest
from unittest.mock import patch
from types import SimpleNamespace

ROOT = Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location('local_windows_ci', ROOT / 'scripts/ci/local-windows.py')
WINDOWS = importlib.util.module_from_spec(SPEC)
with patch.object(sys, 'path', [str(ROOT / 'scripts/ci'), *sys.path]):
    SPEC.loader.exec_module(WINDOWS)


def rows(steps, **fields):
    job = {'runs-on': 'windows-2025', 'steps': steps, **fields}
    return WINDOWS.plan({'jobs': {'windows-tests': job}})[0]['steps']


class WorkflowBodies(unittest.TestCase):
    def test_mixed_infrastructure_keeps_its_actual_body(self):
        body = WINDOWS.CI.RUST_CACHE.format(toolchain='') + '\nexit 19\n'
        result = rows([{'name': 'Rust cache identity', 'run': body}])[0]
        self.assertEqual(result['body'], body)
        self.assertEqual(result['status'], 'not run')
        mixed = rows([{'uses': 'actions/checkout@fixed', 'run': 'exit 19'}])[0]
        self.assertNotIn('body', mixed)
        self.assertNotEqual(mixed['status'], 'local')

    def test_owner_machine_exclusions_are_unexecuted(self):
        for body in ['python scripts/ci/outside-job.py --custody-only -- true',
                     'bash scripts/ci/windows-next-shell.sh',
                     'bash scripts/ci/client-native.sh windows core',
                     'python scripts/ci/ensure-webview2.py']:
            with self.subTest(body=body):
                result = rows([{'run': body}])[0]
                self.assertNotIn('body', result)
                self.assertEqual(result['status'], 'not run')

    def test_only_the_fixed_workspace_binding_is_accepted(self):
        step = {'run': 'cargo build', 'env': {'CARGO_TARGET_DIR': '${{ github.workspace }}/target/windows-next'}}
        self.assertIn('body', rows([step])[0])
        step['env']['CARGO_TARGET_DIR'] = '${{ secrets.other }}'
        self.assertNotIn('body', rows([step])[0])
        step['env'] = {'HGS_CONFIG_DIR': 'owner'}
        self.assertNotIn('body', rows([step])[0])

    def test_stale_or_replaced_executables_do_not_satisfy_a_producer(self):
        with tempfile.TemporaryDirectory() as directory:
            target = Path(directory); binary = target / 'hgs.exe'; binary.write_bytes(b'original')
            record = {'commit': 'a' * 40, 'files': {'hgs.exe': WINDOWS.CI.file_digest(binary)}}
            self.assertTrue(WINDOWS.producer_ready(record, 'a' * 40, target))
            self.assertFalse(WINDOWS.producer_ready(record, 'b' * 40, target))
            binary.write_bytes(b'replaced')
            self.assertFalse(WINDOWS.producer_ready(record, 'a' * 40, target))
            binary.unlink()
            self.assertFalse(WINDOWS.producer_ready(record, 'a' * 40, target))
            self.assertFalse(WINDOWS.producer_ready(None, 'a' * 40, target))

    def test_commit_checkout_excludes_staged_untracked_and_ignored_shadows(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory); source = root / 'source'; source.mkdir()
            home = root / 'home'; home.mkdir()
            env = dict(os.environ, HOME=str(home), GIT_CONFIG_NOSYSTEM='1',
                       GIT_CONFIG_GLOBAL=os.devnull, GIT_TERMINAL_PROMPT='0')
            def git(*args):
                return subprocess.check_output(['git', '-C', str(source), *args], env=env, text=True).strip()
            git('init', '-q')
            (source / 'check.sh').write_text('exit 19\n')
            (source / '.gitignore').write_text('ignored.sh\nunittest.py\n')
            (source / 'test_required.py').write_text('import unittest\nclass Required(unittest.TestCase):\n def test_required(self):self.fail("committed required failure")\n')
            workflow = source / '.github/workflows/checks.yml'; workflow.parent.mkdir(parents=True)
            workflow.write_text('jobs: {windows-tests: {runs-on: windows-2025, steps: [{run: bash check.sh}]}}\n')
            custody = source / 'scripts/ci/outside-job.py'; custody.parent.mkdir(parents=True)
            custody.write_text('ORIGINAL CUSTODY\n')
            git('add', '.')
            git('-c', 'user.name=Fixture', '-c', 'user.email=fixture@example.test', 'commit', '-qm', 'required failing check')
            commit = git('rev-parse', 'HEAD')
            (source / 'check.sh').write_text('exit 0\n')
            workflow.write_text('jobs: {}\n'); custody.write_text('STAGED SHADOW\n')
            git('add', '.')
            (source / 'untracked.sh').write_text('exit 0\n')
            (source / 'ignored.sh').write_text('exit 0\n')
            (source / 'unittest.py').write_text('from pathlib import Path;Path("shadow-ran").write_text("bad");raise SystemExit(0)\n')
            selected = root / 'snapshot'
            WINDOWS.immutable_checkout(source, commit, selected, env)
            job = root / 'job'
            WINDOWS.immutable_checkout(selected, commit, job, env)
            self.assertEqual(subprocess.run(['bash', 'check.sh'], cwd=job, env=env).returncode, 19)
            self.assertEqual((job / 'scripts/ci/outside-job.py').read_text(), 'ORIGINAL CUSTODY\n')
            self.assertFalse((job / 'untracked.sh').exists())
            self.assertFalse((job / 'ignored.sh').exists())
            checked = subprocess.run([sys.executable, '-m', 'unittest', 'test_required'],
                                     cwd=job, env=env, capture_output=True, text=True)
            self.assertEqual(checked.returncode, 1)
            self.assertIn('committed required failure', checked.stdout + checked.stderr)
            self.assertFalse((job / 'shadow-ran').exists())
            self.assertFalse((job / 'unittest.py').exists())
            original_workflow = subprocess.check_output(['git', '-C', str(source), 'show', commit + ':' + WINDOWS.CI.WORKFLOW])
            try:
                planned = WINDOWS.plan(WINDOWS.CI.load_workflow(original_workflow))
            except ValueError as error:
                self.assertIsInstance(error.__cause__, ImportError)
                self.assertEqual(str(error), 'local runner requires python-yaml (PyYAML); no checks started')
            else:
                self.assertEqual(planned[0]['steps'][0]['body'], 'bash check.sh')
            self.assertEqual((source / 'check.sh').read_text(), 'exit 0\n')

    def test_unassigned_original_worker_gets_eof_without_a_release(self):
        with tempfile.TemporaryDirectory() as directory:
            marker = Path(directory) / 'must-not-run'
            code = ('import sys;from pathlib import Path;token=sys.stdin.buffer.read(1);'
                    'Path('+repr(str(marker))+').write_text("released") if token else None;'
                    'raise SystemExit(125)')
            process = subprocess.Popen([sys.executable, '-c', code], stdin=subprocess.PIPE,
                                       stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            try:
                # An AssignProcessToJobObject refusal leaves this SAME original
                # gated worker outside the job; no utility has been released.
                WINDOWS.settle_original_worker(process)
                self.assertTrue(process.stdin.closed)
                self.assertEqual(process.returncode, 125)
                self.assertFalse(marker.exists())
            finally:
                if process.poll() is None:
                    process.kill(); process.wait(timeout=5)

    def test_successful_wrong_node_major_refuses_before_source_or_checks(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            args = SimpleNamespace(root=root, output=root/'output', repo=root/'repo',
                                   node_directory='wrong-node', until='2030-01-01T00:00:00Z')
            runner = WINDOWS.Runner(args, {'commit': 'a'*40})
            versions = [subprocess.CompletedProcess([], 0, stdout=value) for value in
                        ['Python 3.13.15', 'v22.0.0', 'rustc 1.99.1']]
            with patch.object(WINDOWS, 'vc_environment', return_value={'VCTOOLSINSTALLDIR': 'C:/VS'}), \
                 patch.object(WINDOWS.subprocess, 'run', side_effect=versions), \
                 patch.object(WINDOWS, 'immutable_checkout') as checkout:
                with self.assertRaisesRegex(RuntimeError, 'Node24 required'):
                    runner.prepare()
                checkout.assert_not_called()

    def test_public_computer_name_survives_but_source_authority_environment_does_not(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            args = SimpleNamespace(root=root, output=root/'output', repo=root/'repo',
                                   node_directory='node', until='2030-01-01T00:00:00Z')
            runner = WINDOWS.Runner(args, {})
            runner.sdk = {'VCTOOLSINSTALLDIR': 'C:/VS'}
            with patch.dict(os.environ, {'COMPUTERNAME': 'FIXTURE', 'HGS_STATE_DIR': '/owner',
                                         'ZERUS_CLIENT_DESKTOP_SERVICE': '1', 'TOKEN': 'not-a-credential',
                                         'PYTHONUTF8': '0'}):
                env = runner.environment(root/'home')
            self.assertEqual(env['COMPUTERNAME'], 'FIXTURE')
            self.assertNotIn('HGS_STATE_DIR', env)
            self.assertNotIn('ZERUS_CLIENT_DESKTOP_SERVICE', env)
            self.assertNotIn('TOKEN', env)
            self.assertEqual(env['PYTHONUTF8'], '1')

    @unittest.skipUnless(os.name == 'nt', 'native Windows Python pipe decoding')
    def test_private_python_decodes_utf8_cli_error_output(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            args = SimpleNamespace(root=root, output=root/'output', repo=root/'repo',
                                   node_directory='node', until='2030-01-01T00:00:00Z')
            runner = WINDOWS.Runner(args, {})
            runner.sdk = {'VCTOOLSINSTALLDIR': 'C:/VS'}
            env = runner.environment(root/'home')
            expected = 'missing parent: \u201csynthetic\u201d\n'
            writer = 'import os; os.write(2, bytes.fromhex(' + repr(expected.encode().hex()) + '))'
            probe = ('import subprocess,sys; p=subprocess.run([sys.executable,"-c",' + repr(writer)
                     + '],capture_output=True,text=True); assert p.returncode==0; assert p.stderr.endswith('
                     + repr(expected) + ')')
            result = subprocess.run([sys.executable, '-c', probe], env=env,
                                    capture_output=True, text=True, errors='replace', timeout=10)
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)

    def test_conditions_and_tool_versions_never_guess(self):
        self.assertNotIn('body', rows([{'run': 'true', 'if': 'new_condition'}])[0])
        result = rows([{'uses': 'actions/setup-python@fixed', 'with': {'python-version': '3.14'}}])[0]
        self.assertEqual(result['status'], 'not run')
        jobs = WINDOWS.plan({'jobs': {'new-windows-job': {'runs-on': 'windows-2025', 'steps': [{'run': 'true'}]}}})
        self.assertEqual(jobs[0]['status'], 'not run')


if __name__ == '__main__':
    unittest.main()
