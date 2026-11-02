"""Disposable Windows CI: actual own-user task, never start/stop a service."""
import base64
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


HGS = Path(sys.argv[sys.argv.index('--hgs') + 1]).resolve() if '--hgs' in sys.argv else None
if '--hgs' in sys.argv:
    index = sys.argv.index('--hgs')
    del sys.argv[index:index + 2]


@unittest.skipUnless(os.name == 'nt' and HGS is not None, 'explicit disposable Windows CLI required')
class WindowsDesktopRegistration(unittest.TestCase):
    def test_inherited_module_autoload_versus_explicit_minimal_environment(self):
        # A controlled fresh-profile experiment, not a replay of a stuck PC
        # operation. The only user module is this private synthetic probe.
        root = Path(tempfile.mkdtemp(prefix='zerus-registration-environment-'))
        source = Path(__file__).resolve().parents[1] / 'src/desktop/windows_registration.ps1'
        program = Path(os.environ['SystemRoot']) / 'System32/WindowsPowerShell/v1.0/powershell.exe'
        system_modules = program.parent / 'Modules'
        module = root / 'modules' / 'ZerusEnvironmentProbe'
        module.mkdir(parents=True)
        (module / 'ZerusEnvironmentProbe.psm1').write_text(
            "function Invoke-ZerusEnvironmentProbe { return 'loaded' }\n", encoding='utf-8')
        (module / 'ZerusEnvironmentProbe.psd1').write_text(
            "@{RootModule='ZerusEnvironmentProbe.psm1';ModuleVersion='1.0.0';"
            "FunctionsToExport=@('Invoke-ZerusEnvironmentProbe')}\n", encoding='utf-8')
        safe = {'SystemRoot': os.environ['SystemRoot'], 'WINDIR': os.environ['SystemRoot'],
                'COMSPEC': str(program.parents[2] / 'cmd.exe'), 'PATH': str(program.parents[2]),
                'TEMP': str(root), 'TMP': str(root)}
        escaped = str(source).replace("'", "''")
        script = ("$ErrorActionPreference='Stop';$ProgressPreference='SilentlyContinue';"
                  "[Console]::OutputEncoding=[Text.UTF8Encoding]::new($false);"
                  ". ([ScriptBlock]::Create([IO.File]::ReadAllText('" + escaped + "')));"
                  "if($env:ZERUS_TEST_PARENT_PROFILE){$PSModuleAutoLoadingPreference='All'}"
                  "else{Initialize-RegistrationEnvironment};"
                  "try{$probe=Invoke-ZerusEnvironmentProbe}"
                  "catch [System.Management.Automation.CommandNotFoundException]{$probe='refused'};"
                  "[Console]::WriteLine((Get-RegistrationJson).Serialize([ordered]@{"
                  "probe=$probe;autoload=[string]$PSModuleAutoLoadingPreference;"
                  "module_path=$env:PSModulePath;parent_marker=$env:ZERUS_TEST_PARENT_PROFILE}))")
        command = [str(program), '-NoLogo', '-NoProfile', '-NonInteractive', '-EncodedCommand',
                   base64.b64encode(script.encode('utf-16le')).decode('ascii')]
        pending = False
        results = {}
        try:
            for mode in ('inherited', 'minimal'):
                env = dict(safe, PSModulePath=str(root / 'modules') if mode == 'inherited' else str(system_modules))
                if mode == 'inherited':
                    env['ZERUS_TEST_PARENT_PROFILE'] = 'controlled-parent'
                with (root / (mode + '.out')).open('wb') as out, (root / (mode + '.err')).open('wb') as err:
                    started = time.monotonic()
                    process = subprocess.Popen(command, env=env, stdin=subprocess.DEVNULL,
                                               stdout=out, stderr=err, creationflags=subprocess.CREATE_NO_WINDOW)
                    exited = False
                    try:
                        code = process.wait(timeout=15)
                        exited = True
                    except subprocess.TimeoutExpired:
                        try:
                            process.kill()
                        except OSError:
                            pass  # A failed request still requires the original physical wait.
                        try:
                            process.wait(timeout=2)
                            exited = True
                        except subprocess.TimeoutExpired:
                            pending = True
                        raise
                    finally:
                        pending = pending or not exited
                    with (root / (mode + '.err')).open('rb') as error:
                        self.assertEqual(code, 0, error.read(4096))
                with (root / (mode + '.out')).open('rb') as captured:
                    output = captured.read(65537)
                self.assertLessEqual(len(output), 65536)
                value = json.loads(output)
                value['elapsed_ms'] = round((time.monotonic() - started) * 1000)
                results[mode] = value
                print('registration environment experiment:', mode, json.dumps(value), flush=True)
            self.assertEqual(results['inherited']['probe'], 'loaded')
            self.assertEqual(results['inherited']['autoload'], 'All')
            self.assertEqual(results['inherited']['parent_marker'], 'controlled-parent')
            self.assertIn(str(root / 'modules'), results['inherited']['module_path'])
            self.assertEqual(results['minimal']['probe'], 'refused')
            self.assertEqual(results['minimal']['autoload'], 'None')
            self.assertIsNone(results['minimal']['parent_marker'])
            self.assertEqual(results['minimal']['module_path'].casefold(), str(system_modules).casefold())
        finally:
            if pending:
                print('Environment experiment termination pending; original PID/root retained:',
                      process.pid, root, file=sys.stderr)
            else:
                shutil.rmtree(root)

    def test_cold_notification_entry_refuses_without_loading_configuration(self):
        with tempfile.TemporaryDirectory(prefix='zerus-toast-cold-') as directory:
            root = Path(directory)
            env = {key: os.environ[key] for key in ('SystemRoot', 'WINDIR', 'COMSPEC') if key in os.environ}
            env.update(HGS_CONFIG_DIR=str(root / 'unread-config'), HGS_STATE_DIR=str(root / 'unread-state'))
            result = subprocess.run([str(HGS), '--desktop-notification-activation', '-Embedding'],
                                    env=env, capture_output=True, timeout=15)
            self.assertEqual(result.returncode, 78, result.stderr)
            self.assertEqual(result.stdout, b'')
            self.assertEqual(list(root.iterdir()), [])

    def test_fixed_input_frame_finishes_while_the_writer_stays_open(self):
        source = Path(__file__).resolve().parents[1] / 'src/desktop/windows_registration.ps1'
        script = str(source).replace("'", "''")
        body = json.dumps({'schema': 1, 'text': 'framed request'}).encode('utf-8')
        script = ("$ProgressPreference='SilentlyContinue';. ([ScriptBlock]::Create([IO.File]::ReadAllText('" + script + "')))\n"
                  "$s=[Console]::OpenStandardInput();"
                  "try { $v=Read-RegistrationRequest $s " + str(len(body)) + ";"
                  "[Console]::WriteLine((Get-RegistrationJson).Serialize($v)) } finally {$s.Dispose()}")
        program = Path(os.environ['SystemRoot']) / 'System32/WindowsPowerShell/v1.0/powershell.exe'
        process = subprocess.Popen([str(program), '-NoLogo', '-NoProfile', '-NonInteractive',
                                    '-EncodedCommand', base64.b64encode(script.encode('utf-16le')).decode('ascii')],
                                   stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                                   creationflags=subprocess.CREATE_NO_WINDOW)
        try:
            process.stdin.write(body)
            process.stdin.flush()
            # Deliberately keep the ORIGINAL writer open, with no EOF. A timed
            # out reader fails this test; the 15s product limit is unchanged.
            self.assertEqual(process.wait(timeout=15), 0)
            self.assertFalse(process.stdin.closed)
            output = process.stdout.read(65537)
            error = process.stderr.read(65537)
            self.assertLessEqual(len(output), 65536)
            self.assertEqual(error, b'')
            self.assertEqual(json.loads(output), json.loads(body))
        finally:
            process.stdin.close()
            if process.poll() is None:
                process.kill()
            process.wait(timeout=5)
            process.stdout.close()
            process.stderr.close()

    def test_own_interactive_logon_task_without_starting_service_or_enabling_sessions(self):
        root = Path(tempfile.mkdtemp(prefix='zerus-desktop-logon-'))
        env = {key: os.environ[key] for key in ('SystemRoot', 'WINDIR', 'COMSPEC') if key in os.environ}
        env.update(HOME=str(root), USERPROFILE=str(root),
                   APPDATA=str(root / 'roaming'), LOCALAPPDATA=str(root / 'local'),
                   TEMP=str(root / 'tmp'), TMP=str(root / 'tmp'),
                   HGS_CONFIG_DIR=str(root / 'config'), HGS_STATE_DIR=str(root / 'state'),
                   HGS_SELF='windows-registration-fixture', HGS_PEERS='')
        if os.environ.get('HGS_DESKTOP_REGISTRATION_TRACE') == '1':
            env['HGS_DESKTOP_REGISTRATION_TRACE'] = '1'
        for name in ('roaming', 'local', 'tmp', 'config'):
            (root / name).mkdir()

        def cli(*args):
            result = subprocess.run([str(HGS), *args], env=env, capture_output=True,
                                    text=True, encoding='utf-8', timeout=75)
            self.assertEqual(result.returncode, 0, result.stderr + result.stdout)
            self.assertLessEqual(len(result.stdout.encode('utf-8')), 65536)
            return json.loads(result.stdout)

        def digest(path):
            self.assertLessEqual(path.stat().st_size, 256 * 1024 * 1024)
            with path.open('rb') as stream:
                return hashlib.file_digest(stream, 'sha256').digest()

        absent = False
        installed = False
        try:
            status = cli('desktop', 'registration', 'status', '--json')
            self.assertFalse(status['registered'], 'Keep pre-existing task; this runner is not disposable')
            absent = True
            installed = True
            absent = False
            first = cli('desktop', 'registration', 'install', '--json')
            self.assertTrue(first['registered'])
            self.assertTrue(first['changed'])
            self.assertFalse(first['started'])
            self.assertFalse(first['stopped'])
            self.assertEqual(first['task_state'], 3)  # TASK_STATE_READY, not queued/running.
            staged = root / '.local' / 'bin' / 'hgs.exe'
            self.assertEqual(digest(staged), digest(HGS))
            self.assertFalse((root / 'config' / 'local-sessions').exists())
            self.assertFalse(cli('desktop', 'status', '--json')['running'])
            again = cli('desktop', 'registration', 'install', '--json')
            self.assertFalse(again['changed'])
            self.assertFalse(again['cli']['changed'])
            self.assertEqual(again['task_state'], 3)
            removed = cli('desktop', 'registration', 'remove', '--json')
            self.assertTrue(removed['changed'])
            self.assertFalse(removed['registered'])
            self.assertFalse(removed['stopped'])
            self.assertFalse(cli('desktop', 'registration', 'status', '--json')['registered'])
            installed = False
            absent = True
            self.assertFalse(cli('desktop', 'registration', 'remove', '--json')['changed'])
            self.assertTrue(staged.exists())
        finally:
            # Only our exact native-validated task can be removed. Uncertainty
            # retains its source paths rather than leaving a dangling task.
            if installed:
                try:
                    current = cli('desktop', 'registration', 'status', '--json')
                    absent = not current['registered']
                    if not absent:
                        absent = not cli('desktop', 'registration', 'remove', '--json')['registered']
                except Exception:
                    absent = False
            if absent:
                shutil.rmtree(root)
            else:
                print('Registration cleanup uncertain; private root retained:', root, file=sys.stderr)


if __name__ == '__main__':
    unittest.main()
