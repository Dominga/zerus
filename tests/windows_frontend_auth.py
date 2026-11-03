"""Explicit Windows native suite, with exact CI-built CLI and frontend fixture.

No WebView or native agent is started. WindowsFrontend uses a private passive
foreground service. WindowsTaskMode additionally exercises its own disposable
hosted-runner logon task and private service configuration.
"""
from contextlib import closing
import json
import os
from pathlib import Path
import shutil
import sqlite3
import subprocess
import sys
import unittest
import uuid
import base64
import tempfile
import test_desktop as kernel


class WindowsFrontend(unittest.TestCase):
    cli = kernel.Desktop.cli
    serve = kernel.Desktop.serve
    stop = kernel.Desktop.stop

    def setUp(self):
        self.assertEqual(os.name, 'nt', 'run this native suite on Windows')
        fixture = Path(os.environ['HGS_WINDOWS_FRONTEND_FIXTURE']).resolve(strict=True)
        original_cli = kernel.HGS
        kernel.Desktop.setUp(self)
        stage = self.root / 'native'
        stage.mkdir()
        selected = stage / 'hgs.exe'
        self.frontend = stage / 'hgs-tray.exe'
        shutil.copy2(original_cli, selected)
        shutil.copy2(fixture, self.frontend)
        kernel.HGS = selected
        self.addCleanup(setattr, kernel, 'HGS', original_cli)
        self.env.update(LOCALAPPDATA=str(self.root / 'AppData' / 'Local'))
        self.control = self.root / 'control.json'

    def invoke(self, case='positive'):
        _, status = self.serve()
        request_id = str(uuid.uuid4())
        self.control.write_text(json.dumps(dict(case=case, configuration=status['configuration'],
                                               cli=str(kernel.HGS), request_id=request_id)), encoding='utf-8')
        env = dict(self.env, HGS_FRONTEND_TEST_CONTROL=str(self.control))
        if case == 'huge_env':
            for index in range(3):
                env[f'ZERUS_FRONTEND_FIXTURE_PAD{index}'] = 'λ' * 20_000
        output = subprocess.run([str(self.frontend), '--exact',
                                 'instance::tests::evidence::native_frontend_request_fixture', '--nocapture'],
                                env=env, capture_output=True, text=True, timeout=15,
                                creationflags=subprocess.CREATE_NO_WINDOW)
        self.assertEqual(output.returncode, 0, (output.stdout + output.stderr)[-8000:])
        results = [json.loads(line.removeprefix('FRONTEND_RESULT ')) for line in output.stdout.splitlines()
                   if line.startswith('FRONTEND_RESULT ')]
        self.assertEqual(len(results), 1, output.stdout[-4096:])
        chains = [json.loads(line.removeprefix('FRONTEND_CHAIN ')) for line in output.stdout.splitlines()
                  if line.startswith('FRONTEND_CHAIN ')]
        self.assertEqual(len(chains), 1, output.stdout[-4096:])
        self.assertLessEqual(len(chains[0]), 32)
        print('FRONTEND_OBSERVED_CHAIN ' + json.dumps(dict(case=case, chain=chains[0])), file=sys.stderr)
        self.assertEqual(results[0]['request_id'], request_id)
        if case != 'positive':
            self.assertFalse(results[0]['accepted'])
            self.assertFalse(results[0]['durable'])
        # Safe passive OFF may commit; neither positive nor negative case can
        # enable approvals or grant writers in this privately launched service.
        after = self.cli('desktop', 'status', '--json')
        self.assertFalse(after['writers_enabled'])
        self.assertFalse(after['automatic_approvals'])
        self.assertFalse((self.root / 'unexpected-tool').exists())
        return results[0]

    def mode_check(self, image=None):
        self.control.write_text(json.dumps(dict(case='mode_check', cli=str(kernel.HGS))), encoding='utf-8')
        output = subprocess.run([str(image or self.frontend), '--exact',
                                 'instance::tests::evidence::native_frontend_request_fixture', '--nocapture'],
                                env=dict(self.env, HGS_FRONTEND_TEST_CONTROL=str(self.control)),
                                capture_output=True, text=True, timeout=15,
                                creationflags=subprocess.CREATE_NO_WINDOW)
        self.assertEqual(output.returncode, 0, (output.stdout + output.stderr)[-8000:])
        results = [json.loads(line.removeprefix('MODE_CALLER_RESULT ')) for line in output.stdout.splitlines()
                   if line.startswith('MODE_CALLER_RESULT ')]
        self.assertEqual(len(results), 1, output.stdout[-4096:])
        self.assertFalse(self.desktop.exists(), 'mode admission is read-only')
        return results[0]

    def test_mode_caller_accepts_the_actual_retained_installed_frontend_without_a_console(self):
        result = self.mode_check()
        self.assertEqual(result['code'], 0, result)
        value = json.loads(result['stdout'])
        self.assertTrue(value['admitted'])
        self.assertEqual(value['source'], 'installed_frontend')

    def test_mode_caller_still_refuses_the_real_marked_ancestor_when_the_child_is_clean(self):
        key = 'HGS_RUN_ID'
        previous = os.environ.get(key)
        os.environ[key] = 'native-mode-parent-fixture'
        try:
            result = self.mode_check()
            self.assertNotEqual(result['code'], 0)
            self.assertIn('agent-descended', result['stderr'])
        finally:
            if previous is None:
                os.environ.pop(key, None)
            else:
                os.environ[key] = previous

    def test_mode_caller_refuses_an_agent_named_frontend_image(self):
        agent = self.frontend.with_name('codex.exe')
        shutil.copy2(self.frontend, agent)
        result = self.mode_check(agent)
        self.assertNotEqual(result['code'], 0)
        self.assertIn('agent-descended', result['stderr'])

    def test_checking_cli_cannot_be_its_own_frontend_and_cannot_manufacture_a_console(self):
        shutil.copy2(kernel.HGS, self.frontend)
        result = subprocess.run([str(self.frontend), 'desktop', 'registration', 'mode-check', '--json'],
                                env=self.env, capture_output=True, text=True, timeout=8,
                                creationflags=subprocess.CREATE_NO_WINDOW)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('Settings', result.stderr)
        self.assertFalse(self.desktop.exists())

    def test_real_canonical_frontend_and_original_child_disable_commits_in_passive_service(self):
        receipt = self.invoke()
        self.assertIs(receipt['accepted'], True)
        self.assertIs(receipt['durable'], True)
        self.assertIs(receipt['enabled'], False)
        self.assertEqual(receipt, dict(request_id=receipt['request_id'], accepted=True, durable=True,
                                      op='automatic_approvals', enabled=False, revision=1))
        database = self.desktop / 'state.sqlite3'
        self.assertTrue(database.is_file())
        with closing(sqlite3.connect(database.as_uri() + '?mode=ro', uri=True)) as db:
            recorded = db.execute('SELECT receipt FROM desktop_delivery_requests WHERE request_id=?',
                                  (receipt['request_id'],)).fetchone()
            self.assertIsNotNone(recorded)
            self.assertEqual(json.loads(recorded[0]), receipt)
            self.assertEqual(db.execute('SELECT enabled,revision FROM desktop_delivery_policy WHERE id=1')
                             .fetchone(), (0, 1))

    def test_authenticated_enable_still_refuses_in_passive_service(self):
        self.assertEqual(self.invoke('enable')['code'], 'desktop_background_unavailable')

    def test_wrong_native_child_birth_refuses_before_actor_admission(self):
        self.assertEqual(self.invoke('wrong_birth')['code'], 'frontend_authentication_refused')

    def test_wrong_cached_configuration_is_not_native_authority(self):
        self.assertEqual(self.invoke('wrong_config')['code'], 'frontend_authentication_refused')

    def test_wrong_native_request_fingerprint_refuses_before_actor_admission(self):
        self.assertEqual(self.invoke('wrong_fingerprint')['code'], 'frontend_authentication_refused')

    def test_oversize_unicode_environment_remains_unavailable(self):
        self.assertEqual(self.invoke('huge_env')['code'], 'frontend_authentication_refused')

    def test_child_stripping_markers_cannot_hide_its_real_tagged_ancestor(self):
        # Only this disposable test driver is tagged. The explicit child env
        # remains clean; the service must inspect the original parent chain.
        key = 'HGS_RUN_ID'
        previous = os.environ.get(key)
        os.environ[key] = 'native-fixture-parent'
        try:
            self.assertEqual(self.invoke('agent_parent')['code'], 'frontend_authentication_refused')
        finally:
            if previous is None:
                os.environ.pop(key, None)
            else:
                os.environ[key] = previous


@unittest.skipUnless(os.name == 'nt' and os.environ.get('GITHUB_ACTIONS') == 'true'
                     and os.environ.get('RUNNER_ENVIRONMENT') == 'github-hosted',
                     'Task-mode fixture requires a disposable hosted Windows runner')
class WindowsTaskMode(unittest.TestCase):
    def setUp(self):
        self.root = Path(tempfile.mkdtemp(prefix='zerus-task-mode-')).resolve()
        self.cli_image = self.root / 'portable' / 'hgs.exe'
        self.cli_image.parent.mkdir()
        shutil.copy2(kernel.HGS, self.cli_image)
        self.profile = self.root / 'profile'
        self.profile.mkdir()
        self.frontend = self.profile / '.local' / 'bin' / 'hgs-tray.exe'
        self.frontend_source = Path(os.environ['HGS_WINDOWS_FRONTEND_FIXTURE']).resolve(strict=True)
        self.config = self.root / 'config'
        self.config.mkdir()
        (self.config / 'config').write_text('HGS_SELF=windows-mode-fixture\nHGS_PEERS=\n', encoding='utf-8')
        self.state = self.root / 'state'
        self.control = self.root / 'control.json'
        self.env = {k:v for k,v in os.environ.items() if not k.upper().startswith('HGS_')}
        # HOME selects a wholly private staging profile. USERPROFILE stays
        # real solely for the canonical native instance namespace shared with
        # the Scheduler-launched service; it is not a caller/ownership permit.
        profile = os.environ['USERPROFILE']
        self.env.update(HOME=str(self.profile), USERPROFILE=profile, HGS_CONFIG_DIR=str(self.config),
                        HGS_STATE_DIR=str(self.state), HGS_PEERS='')
        self.task = None
        self.installed = False
        self.latest = None

    def cli(self, *args):
        result = subprocess.run([str(self.cli_image), *args], env=self.env,
                                capture_output=True, text=True, encoding='utf-8', timeout=20)
        self.assertEqual(result.returncode, 0, (result.stderr + result.stdout)[-8192:])
        self.assertLessEqual(len(result.stdout.encode('utf-8')), 65536)
        return json.loads(result.stdout)

    def frontend_call(self, control, prefix):
        self.control.write_text(json.dumps(dict(control, cli=str(self.cli_image))), encoding='utf-8')
        result = subprocess.run([str(self.frontend), '--exact',
                                 'instance::tests::evidence::native_frontend_request_fixture', '--nocapture'],
                                env=dict(self.env, HGS_FRONTEND_TEST_CONTROL=str(self.control)),
                                capture_output=True, text=True, timeout=40,
                                creationflags=subprocess.CREATE_NO_WINDOW)
        self.assertEqual(result.returncode, 0, (result.stdout + result.stderr)[-8192:])
        values = [json.loads(line.removeprefix(prefix)) for line in result.stdout.splitlines()
                  if line.startswith(prefix)]
        self.assertEqual(len(values), 1, result.stdout[-8192:])
        return values[0]

    def mode(self, desired):
        result = self.frontend_call(dict(case='mode_switch', desired=desired), 'MODE_CALLER_RESULT ')
        self.assertEqual(result['code'], 0, result)
        return json.loads(result['stdout'])

    def grant(self):
        status = self.cli('desktop', 'status', '--json')
        ownership = self.cli('desktop', 'ownership-status', '--json')
        result = self.frontend_call(dict(case='ownership_grant', configuration=status['configuration'],
                                        journal=ownership['journal'], revision=ownership['revision'],
                                        request_id=str(uuid.uuid4())), 'FRONTEND_RESULT ')
        self.assertTrue(result['accepted'], result)
        self.assertTrue(result['durable'], result)
        self.assertEqual(result['outcome']['state'], 'granted', result)

    def cancel(self, request_id):
        result = self.frontend_call(dict(case='mode_cancel', request_id=request_id), 'MODE_CALLER_RESULT ')
        self.assertEqual(result['code'], 0, result)
        return json.loads(result['stdout'])

    def recover_initial_start_and_cancel_background_before_a_grant(self, passive):
        path = self.state / 'desktop' / 'windows-registration-mode.json'
        record = json.loads(path.read_text(encoding='utf-8'))
        # Arrange an actual successful start whose reply was lost. The SAME
        # native instance must settle through the helper; no file reset follows.
        request_id = str(uuid.uuid4())
        record['pending'] = dict(request_id=request_id, desired='passive', phase='start_offered',
                                 original=None, started=passive['instance']['guid'])
        path.write_text(json.dumps(record), encoding='utf-8')
        settled = self.mode('passive')
        self.assertTrue(settled['ok'], settled)
        self.assertEqual(settled['code'], 'original_start_observed')
        self.assertEqual(settled['request_id'], request_id)
        self.assertEqual(settled['instance'], passive['instance'])
        self.assertFalse(settled['manager_changed'])
        self.assertIsNone(json.loads(path.read_text(encoding='utf-8'))['pending'])
        self.assertEqual(self.cli('desktop', 'ownership-status', '--json')['state'], 'passive')
        # A reboot/restart may invalidate the original tuple before any manager
        # effect. Cancellation of either background phase requires no grant.
        for phase in ('prepared', 'held'):
            record = json.loads(path.read_text(encoding='utf-8'))
            identity = dict(passive['instance'], birth=passive['instance']['birth'] + 1)
            request_id = str(uuid.uuid4())
            record['pending'] = dict(request_id=request_id, desired='background', phase=phase,
                                     original=identity, started=None)
            path.write_text(json.dumps(record), encoding='utf-8')
            receipt = self.cancel(request_id)
            self.assertTrue(receipt['ok'], receipt)
            self.assertFalse(receipt['manager_changed'])
            self.assertEqual(receipt['cancelled_request_id'], request_id)
        self.assertEqual(self.cli('desktop', 'status', '--json')['lifecycle']['pid'], passive['instance']['pid'])
        return settled

    def metadata_refusals(self, passive):
        # Only fixture metadata is arranged, never native custody. Every wrong
        # original refuses; supported record-only cancellation replaces resets.
        path = self.state / 'desktop' / 'windows-registration-mode.json'
        pid = passive['instance']['pid']
        for field in ('guid', 'pid', 'birth', 'epoch'):
            record = json.loads(path.read_text(encoding='utf-8'))
            identity = dict(passive['instance'])
            identity[field] = str(uuid.uuid4()) if field in ('guid', 'epoch') else identity[field] + 1
            pending = dict(request_id=str(uuid.uuid4()), desired='passive', phase='prepared',
                           original=identity, started=None)
            record['pending'] = pending
            path.write_text(json.dumps(record), encoding='utf-8')
            reply = self.mode('passive')
            self.assertFalse(reply['ok'], reply)
            self.assertIn('Original task or service changed', reply['error'])
            self.assertEqual(reply['pending_transition']['request_id'], pending['request_id'])
            receipt = self.cancel(pending['request_id'])
            self.assertTrue(receipt['ok'], receipt)
            self.assertFalse(receipt['manager_changed'])
            self.assertEqual(self.cli('desktop', 'status', '--json')['lifecycle']['pid'], pid)
        with closing(sqlite3.connect((self.state / 'desktop' / 'state.sqlite3').as_uri() + '?mode=ro', uri=True)) as db:
            self.assertEqual(db.execute('SELECT count(*) FROM desktop_commands').fetchone()[0], 0)

    def busy_exclusion_does_not_reserve(self, passive):
        import msvcrt
        path = self.state / 'desktop' / 'windows-registration-mode.json'
        before = path.read_bytes()
        # The fixture holds byte zero of the SAME private handoff file; fs2's
        # exclusive LockFileEx range includes it. No actor/manager is stopped.
        with (self.state / 'desktop' / 'handoff.lock').open('r+b') as file:
            file.seek(0)
            msvcrt.locking(file.fileno(), msvcrt.LK_NBLCK, 1)
            try:
                reply = self.mode('background')
                self.assertFalse(reply['ok'], reply)
                self.assertIsNone(reply['pending_transition'])
                self.assertEqual(path.read_bytes(), before)
            finally:
                file.seek(0)
                msvcrt.locking(file.fileno(), msvcrt.LK_UNLCK, 1)
        self.assertEqual(self.cli('desktop', 'status', '--json')['lifecycle']['pid'], passive['instance']['pid'])

    def cancel_unsent_private_rollback(self, passive):
        record_path = self.state / 'desktop' / 'windows-registration-mode.json'
        record = json.loads(record_path.read_text(encoding='utf-8'))
        request_id = str(uuid.uuid4())
        record['pending'] = dict(request_id=request_id, desired='passive', phase='revoke_offered',
                                 original=passive['instance'], started=None)
        record_path.write_text(json.dumps(record), encoding='utf-8')
        before = self.cli('desktop', 'ownership-status', '--json')
        result = self.frontend_call(dict(case='mode_cancel', request_id=request_id), 'MODE_CALLER_RESULT ')
        self.assertEqual(result['code'], 0, result)
        receipt = json.loads(result['stdout'])
        self.assertTrue(receipt['ok'], receipt)
        self.assertFalse(receipt['manager_changed'])
        self.assertEqual(receipt['cancelled_request_id'], request_id)
        self.assertEqual(self.cli('desktop', 'ownership-status', '--json'), before)
        self.assertEqual(self.cli('desktop', 'status', '--json')['lifecycle']['pid'], passive['instance']['pid'])
        with closing(sqlite3.connect((self.state / 'desktop' / 'state.sqlite3').as_uri() + '?mode=ro', uri=True)) as db:
            self.assertEqual(db.execute('SELECT count(*) FROM desktop_commands').fetchone()[0], 0)
        return request_id

    def powershell(self, script, data):
        # Fixture-only native COM observation/cleanup. No task-name-wide Stop,
        # schtasks /End, PID kill or executable-pattern cleanup is used.
        request = self.root / ('sdk-' + str(uuid.uuid4()) + '.json')
        request.write_text(json.dumps(data), encoding='utf-8')
        escaped = str(request).replace("'", "''")
        body = "$ErrorActionPreference='Stop';$x=Get-Content -Raw -LiteralPath '" + escaped + "'|ConvertFrom-Json;" + script
        command = [str(Path(os.environ['SystemRoot']) / 'System32/WindowsPowerShell/v1.0/powershell.exe'),
                   '-NoLogo', '-NoProfile', '-NonInteractive', '-EncodedCommand',
                   base64.b64encode(body.encode('utf-16le')).decode('ascii')]
        result = subprocess.run(command, env=self.env, capture_output=True, text=True, timeout=12)
        self.assertEqual(result.returncode, 0, result.stderr + result.stdout)
        self.assertLessEqual(len(result.stdout), 8192)
        return json.loads(result.stdout)

    def observe(self):
        return self.powershell(r'''
$s=New-Object -ComObject Schedule.Service;$s.Connect();$t=$s.GetFolder('\').GetTask($x.task);
$items=@();foreach($i in $t.GetInstances(0)) {
 $p=$null;try {$p=[Diagnostics.Process]::GetProcessById($i.EnginePID).ProcessName} catch {}
 $items+=@{guid=$i.InstanceGuid;engine_pid=$i.EnginePID;engine_image=$p;state=[int]$i.State}
};@{instances=$items;state=[int]$t.State}|ConvertTo-Json -Compress -Depth 4
''', dict(task=self.task))

    def stop_exact_passive(self, expected, *, wrong_birth=False):
        return self.powershell(r'''
$s=New-Object -ComObject Schedule.Service;$s.Connect();$t=$s.GetFolder('\').GetTask($x.task);
$d=$t.Definition;
if($d.Actions.Count-ne 1 -or $d.Actions.Item(1).Path-cne $x.image -or $d.Principal.UserId-ne $x.sid) {throw 'fixture task definition changed'};
$values=@($t.GetInstances(0));if($values.Count-ne 1) {throw 'fixture task instance count changed'};
$i=$values[0];$i.Refresh();if($i.InstanceGuid-cne $x.guid -or $i.EnginePID-ne $x.pid) {throw 'fixture original instance changed'};
$p=[Diagnostics.Process]::GetProcessById($i.EnginePID);$handle=$p.Handle;
if($p.HasExited -or $p.StartTime.ToFileTimeUtc()-ne $x.birth -or $p.MainModule.FileName-cne $x.image) {
 @{stopped=$false;reason='original_birth_or_image_changed'}|ConvertTo-Json -Compress;exit 0
};
$i.Refresh();if($i.InstanceGuid-cne $x.guid -or $i.EnginePID-ne $p.Id) {throw 'fixture instance changed before stop'};
$i.Stop();if(-not $p.WaitForExit(5000)) {throw 'fixture original process exit unconfirmed'};
if(@($t.GetInstances(0)).Count-ne 0) {throw 'fixture instance remains'};
@{stopped=$true;original_pid=$p.Id;original_guid=$i.InstanceGuid}|ConvertTo-Json -Compress
''', dict(task=self.task, image=str(self.cli_image), sid=self.sid, guid=expected['guid'],
          pid=expected['pid'], birth=expected['birth'] + (1 if wrong_birth else 0)))

    def test_explicit_mode_round_trip_uses_the_own_task_and_preserves_unrelated_processes(self):
        # Never touch a pre-existing per-user registration on a reused image.
        initial = self.cli('desktop', 'registration', 'status', '--json')
        self.assertFalse(initial['registered'], 'Task-mode fixture requires an unused disposable runner')
        self.task = initial['task_name']
        self.sid = self.task.removeprefix('hgs-desktop-')
        shell = subprocess.Popen([str(Path(os.environ['SystemRoot']) / 'System32/cmd.exe'),
                                  '/D', '/Q', '/C', 'set /p ZERUS_MODE_FIXTURE_WAIT='],
                                 stdin=subprocess.PIPE, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                                 creationflags=subprocess.CREATE_NO_WINDOW)
        clean = False
        try:
            installed = self.cli('desktop', 'registration', 'install', '--json')
            self.installed = True
            # The maintained registration installer stages the stable CLI
            # without enabling local sessions. Native relays must then use
            # THAT image, matching the actual service's file identity.
            self.cli_image = Path(installed['cli']['path']).resolve(strict=True)
            self.assertEqual(self.cli_image, self.profile / '.local' / 'bin' / 'hgs.exe')
            shutil.copy2(self.frontend_source, self.frontend)
            self.assertFalse((self.config / 'local-sessions').exists())
            self.assertFalse(installed['started'])
            self.assertFalse(self.cli('desktop', 'status', '--json')['running'])
            refused = self.mode('background')
            self.assertFalse(refused['ok'], refused)
            self.assertFalse(self.cli('desktop', 'status', '--json')['running'])
            passive = self.mode('passive')
            self.assertTrue(passive['ok'], passive)
            self.latest = passive['instance']
            self.assertEqual(passive['selected_mode'], 'passive')
            self.assertIsNone(shell.poll())
            passive = self.recover_initial_start_and_cancel_background_before_a_grant(passive)
            self.grant()
            granted = self.cli('desktop', 'ownership-status', '--json')
            no_op = self.mode('passive')
            self.assertTrue(no_op['ok'], no_op)
            self.assertFalse(no_op['changed'])
            self.assertEqual(self.cli('desktop', 'ownership-status', '--json'), granted)
            self.assertEqual(self.cli('desktop', 'status', '--json')['lifecycle']['pid'], passive['instance']['pid'])
            self.metadata_refusals(passive)
            self.busy_exclusion_does_not_reserve(passive)
            cancelled = self.cancel_unsent_private_rollback(passive)
            background = self.mode('background')
            self.assertTrue(background['ok'], background)
            self.assertNotEqual(background['request_id'], cancelled)
            self.assertEqual(background['running_mode'], 'background')
            self.latest = background['instance']
            # Ordinary install/refresh preserves selection and the SAME runtime.
            before = self.cli('desktop', 'status', '--json')['lifecycle']['pid']
            refresh = self.cli('desktop', 'registration', 'install', '--json')
            self.assertFalse(refresh['changed'])
            self.assertFalse(refresh['started'])
            self.assertFalse(refresh['stopped'])
            self.assertEqual(refresh['selected_mode'], 'background')
            self.assertEqual(self.cli('desktop', 'status', '--json')['lifecycle']['pid'], before)
            self.assertIsNone(shell.poll())
            rollback = self.mode('passive')
            self.assertTrue(rollback['ok'], rollback)
            self.latest = rollback['instance']
            self.assertEqual(self.cli('desktop', 'ownership-status', '--json')['state'], 'revoked')
            status = self.cli('desktop', 'registration', 'status', '--json')
            self.assertEqual(status['selected_mode'], 'passive')
            self.assertEqual(status['running_mode'], 'passive')
            self.assertIsNone(status['pending_transition'])
            self.assertEqual(status['completed_request_id'], rollback['request_id'])
            self.assertFalse(self.stop_exact_passive(self.latest, wrong_birth=True)['stopped'])
            self.assertTrue(self.cli('desktop', 'status', '--json')['running'])
            self.assertTrue(self.stop_exact_passive(self.latest)['stopped'])
            self.assertFalse(self.cli('desktop', 'status', '--json')['running'])
            self.assertFalse(self.cli('desktop', 'registration', 'remove', '--json')['registered'])
            self.installed = False
            self.assertIsNone(shell.poll())
            clean = True
        finally:
            shell.stdin.write(b'done\n'); shell.stdin.close()
            shell.wait(timeout=5)
            if clean:
                shutil.rmtree(self.root)
            else:
                # An uncertain original instance is never killed or adopted.
                # Preserve its exact paths; disposable VM teardown is the final
                # boundary. The failed test remains failed/unqualified.
                if self.installed:
                    try:
                        print('MODE_TASK_OBSERVATION ' + json.dumps(self.observe()), file=sys.stderr)
                        status = self.cli('desktop', 'status', '--json')
                        print('MODE_TASK_SERVICE ' + json.dumps({key: status.get(key) for key in
                              ('configuration', 'running', 'lifecycle')}), file=sys.stderr)
                    except Exception as error:
                        print('MODE_TASK_OBSERVATION_UNAVAILABLE ' + type(error).__name__, file=sys.stderr)
                print('MODE_TASK_PRIVATE_ROOT_RETAINED ' + str(self.root), file=sys.stderr)


if __name__ == '__main__':
    unittest.main()
