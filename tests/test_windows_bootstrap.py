"""Run the maintained Windows bootstrap source in isolated PowerShell processes.

Windows uses stock 5.1 plus the selected PowerShell, with no module search path.
Other hosts may supply HGS_TEST_POWERSHELL for syntax/scope controls; those runs
do not qualify the native Windows decoder, its pipes or the cold-start budget.
"""
import base64
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]


def shells():
    found = []
    if os.name == 'nt':
        found.append(str(Path(os.environ['SystemRoot']) /
                         'System32/WindowsPowerShell/v1.0/powershell.exe'))
    for candidate in (os.environ.get('HGS_TEST_POWERSHELL'), shutil.which('pwsh')):
        if candidate and all(os.path.normcase(candidate) != os.path.normcase(old) for old in found):
            found.append(candidate)
    return found


SHELLS = shells()


@unittest.skipUnless(SHELLS, 'isolated PowerShell bootstrap controls need PowerShell')
class WindowsBootstrap(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix='zerus-bootstrap-')
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.script = self.root / 'run_codex.ps1'
        self.result = self.root / 'result.json'
        self.imported = self.root / 'imported'
        self.started = self.root / 'helper-started'
        self.env = {key: value for key, value in os.environ.items()
                    if key.upper() in ('SYSTEMROOT', 'WINDIR', 'COMSPEC', 'PATHEXT', 'LANG')}
        for key in ('HOME', 'USERPROFILE', 'APPDATA', 'LOCALAPPDATA', 'TEMP', 'TMP', 'PSModulePath'):
            directory = self.root / key
            directory.mkdir()
            self.env[key] = str(directory)
        self.env.update(PROBE_RESULT=str(self.result), IMPORT_RECORDED=str(self.imported),
                        HELPER_STARTED=str(self.started))
        source = (ROOT / 'src/project_launcher.rs').read_text()
        self.launcher = re.search(r'pub const BOOTSTRAP: &str = r#"(.*?)"#;', source, re.S).group(1)
        self.decoder = (ROOT / 'src/remote_shell/windows.ps1').read_text()

    def run_bootstrap(self, shell, source, setup=''):
        encoded = base64.b64encode((setup + '\n' + source).encode('utf-16-le')).decode()
        return subprocess.run([shell, '-NoLogo', '-NoProfile', '-NonInteractive',
                               '-EncodedCommand', encoded], env=self.env, cwd=self.root,
                              stdin=subprocess.DEVNULL, capture_output=True, timeout=20)

    def import_refusal(self):
        # A function shadows only the import call, before any helper/script.
        return ("function Import-Module { param($Name,$ErrorAction) "
                "if ($Name -ne \"$PSHOME\\Modules\\Microsoft.PowerShell.Utility\" "
                "-or $PSModuleAutoLoadingPreference -ne 'None') { throw 'wrong import' }; "
                "[IO.File]::WriteAllText($env:IMPORT_RECORDED,'fixed import refused'); "
                "throw 'synthetic fixed import refusal' }")

    def test_launcher_restores_prior_presence_and_value_before_original_script(self):
        values = ['', 'a"b', '-name:value', 'Пример', 'C:\\tail\\', '$(literal);&only-data']
        self.script.write_text(
            "$p=$ExecutionContext.SessionState.PSVariable.Get('PSModuleAutoLoadingPreference');"
            "$absent=$null -eq $p;$expected=$env:EXPECTED_AUTOLOAD;"
            "if (($expected -eq 'absent' -and -not $absent) -or "
            "($expected -ne 'absent' -and ($absent -or $p.Value -ne $expected))) "
            "{ throw 'autoload was not restored' };"
            "$value=@{args=@($args);cleared=($null -eq $env:HGS_PROJECT_LAUNCH_DATA)}"
            "|ConvertTo-Json -Depth 5 -Compress;"
            "[IO.File]::WriteAllText($env:PROBE_RESULT,$value,[Text.UTF8Encoding]::new($false));exit 7")
        self.env['HGS_PROJECT_LAUNCH_DATA'] = json.dumps({'script': str(self.script), 'arguments': values})
        for shell in SHELLS:
            for prior in ('absent', 'All', 'ModuleQualified', 'None'):
                with self.subTest(shell=shell, prior=prior):
                    self.result.unlink(missing_ok=True)
                    self.env['EXPECTED_AUTOLOAD'] = prior
                    setup = ("$ExecutionContext.SessionState.PSVariable.Remove('PSModuleAutoLoadingPreference')"
                             if prior == 'absent' else
                             "$ExecutionContext.SessionState.PSVariable.Set('PSModuleAutoLoadingPreference','" + prior + "')")
                    out = self.run_bootstrap(shell, self.launcher, setup)
                    self.assertEqual(out.returncode, 7, out.stderr)
                    self.assertEqual(json.loads(self.result.read_text(encoding='utf-8-sig')),
                                     {'args': values, 'cleared': True})

    def test_launcher_import_failure_never_runs_the_project_script(self):
        self.script.write_text("[IO.File]::WriteAllText($env:HELPER_STARTED,'started');exit 7")
        self.env['HGS_PROJECT_LAUNCH_DATA'] = json.dumps({'script': str(self.script), 'arguments': []})
        for shell in SHELLS:
            with self.subTest(shell=shell):
                self.imported.unlink(missing_ok=True)
                out = self.run_bootstrap(shell, self.launcher, self.import_refusal())
                self.assertEqual(out.returncode, 1)
                self.assertIn(b'synthetic fixed import refusal', out.stderr)
                self.assertTrue(self.imported.exists())
                self.assertFalse(self.started.exists())

    def test_decoder_import_failure_precedes_helper_start(self):
        # Replace only the native start expression with an observable no-effect
        # seam. The complete maintained bootstrap and its catch still execute.
        self.assertEqual(self.decoder.count('$p.Start()'), 1)
        source = self.decoder.replace('__HGS_ARGUMENTS__', base64.b64encode(b'[]').decode())
        source = source.replace('$p.Start()',
                                "[bool]([IO.File]::WriteAllText($env:HELPER_STARTED,'started'))")
        for shell in SHELLS:
            with self.subTest(shell=shell):
                self.imported.unlink(missing_ok=True)
                out = self.run_bootstrap(shell, source, self.import_refusal())
                self.assertEqual(out.returncode, 127, out.stderr)
                self.assertIn(b'remote Windows invocation failed', out.stderr)
                self.assertTrue(self.imported.exists())
                self.assertFalse(self.started.exists())

    def test_malformed_payload_refuses_before_script_or_helper(self):
        self.script.write_text("[IO.File]::WriteAllText($env:HELPER_STARTED,'started');exit 7")
        self.env['HGS_PROJECT_LAUNCH_DATA'] = '{not-json'
        source = self.decoder.replace('__HGS_ARGUMENTS__', base64.b64encode(b'{not-json').decode())
        self.assertEqual(source.count('$p.Start()'), 1)
        source = source.replace('$p.Start()',
                                "[bool]([IO.File]::WriteAllText($env:HELPER_STARTED,'started'))")
        for shell in SHELLS:
            for bootstrap, expected in ((self.launcher, 1), (source, 127)):
                with self.subTest(shell=shell, expected=expected):
                    out = self.run_bootstrap(shell, bootstrap)
                    self.assertEqual(out.returncode, expected, out.stderr)
                    self.assertFalse(self.started.exists())

    def test_decoder_initialization_uses_fixed_module_without_search_cmdlets(self):
        # Evaluate the actual initializer, stopping before native child creation.
        initializer = self.decoder.split('$pipe=[Console]::IsInputRedirected;', 1)[0]
        self.assertNotIn('Join-Path', initializer)
        self.assertNotIn('ForEach-Object', initializer)
        values = ['', 'a"b', 'back\\', '-name:value', '日本語', '$(literal);&only-data']
        initializer = initializer.replace('__HGS_ARGUMENTS__', base64.b64encode(json.dumps(values).encode()).decode())
        source = initializer + ("$value=@{args=@($a);program=$i.FileName;"
                                "preference=$PSModuleAutoLoadingPreference}|ConvertTo-Json -Compress;"
                                "[IO.File]::WriteAllText($env:PROBE_RESULT,$value,[Text.UTF8Encoding]::new($false));"
                                "exit 0 } catch { [Console]::Error.WriteLine($_.ToString());exit 1 }")
        for shell in SHELLS:
            with self.subTest(shell=shell):
                out = self.run_bootstrap(shell, source)
                self.assertEqual(out.returncode, 0, out.stderr)
                value = json.loads(self.result.read_text(encoding='utf-8-sig'))
                self.assertEqual(value['args'], values)
                self.assertEqual(value['preference'], 'None')
                self.assertEqual(value['program'], str(Path(self.env['USERPROFILE']) / r'.local\bin\hgs.exe'))


if __name__ == '__main__':
    unittest.main()
