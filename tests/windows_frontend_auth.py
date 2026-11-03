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
import time
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

    def test_registered_startup_refusal_retains_the_guard_reason_without_replacing_the_owner(self):
        original, status = self.serve()
        result = self.cli('desktop', 'serve', '--config-dir', str(self.config),
                          '--state-dir', str(self.state), ok=False)
        self.assertEqual(result.returncode, 78, result.stderr)
        path = self.desktop / 'startup-failure.json'
        data = path.read_bytes()
        self.assertLessEqual(len(data), 4096)
        diagnostic = json.loads(data)
        self.assertEqual(diagnostic['abi'], 'desktop-startup-failure-v1')
        self.assertEqual(diagnostic['exit_code'], 78)
        self.assertEqual(diagnostic['configuration'], status['configuration'])
        self.assertIn('acquire desktop owner lock', diagnostic['message'])
        self.assertIn('another desktop service owns this state', diagnostic['message'])
        self.assertIsNone(original.poll())
        self.assertEqual(self.cli('desktop', 'status', '--json')['lifecycle']['pid'], original.pid)

    def test_registered_passive_start_without_a_console_keeps_the_original_startup_reason(self):
        # Separate private configuration: this diagnoses the no-console launch
        # class before any Scheduler test, never retries an uncertain RunEx.
        self.desktop.mkdir(parents=True)
        original = subprocess.Popen([str(kernel.HGS), 'desktop', 'serve', '--config-dir', str(self.config),
                                     '--state-dir', str(self.state)], env=self.env,
                                    stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                                    text=True, creationflags=subprocess.CREATE_NO_WINDOW)
        self.addCleanup(self.stop, original)
        until = time.monotonic() + 10
        while time.monotonic() < until:
            if original.poll() is not None:
                _, stderr = original.communicate(timeout=1)
                self.fail('private no-console registered startup refused: ' + stderr[-4096:])
            status = self.cli('desktop', 'status', '--json')
            if status.get('lifecycle', {}).get('ipc_ready'):
                self.assertEqual(status['lifecycle']['pid'], original.pid)
                self.assertEqual(status['lifecycle']['mode'], 'passive')
                return
            time.sleep(.02)
        self.fail('private no-console registered startup did not reach readiness')

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

    def grant(self, case='ownership_grant'):
        status = self.cli('desktop', 'status', '--json')
        ownership = self.cli('desktop', 'ownership-status', '--json')
        result = self.frontend_call(dict(case=case, configuration=status['configuration'],
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
        request_id = passive['request_id']
        self.assertEqual(record['completed'], request_id)
        before = self.observe()
        record['completed'] = None  # original completion reply/save was lost
        record['pending'] = dict(request_id=request_id, desired='passive', phase='start_offered',
                                 original=None, started=passive['instance']['guid'])
        path.write_text(json.dumps(record), encoding='utf-8')
        settled = self.mode('passive')
        self.assertTrue(settled['ok'], settled)
        self.assertEqual(settled['code'], 'original_start_observed')
        self.assertEqual(settled['request_id'], request_id)
        self.assertEqual(settled['instance'], passive['instance'])
        self.assertFalse(settled['manager_changed'])
        self.assertEqual(self.observe(), before, 'same GUID/process/start time; no manager replay')
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

    def cancel_ready_rollback_after_authority_moves(self, background, rollback, *, later_revoke=False):
        path = self.state / 'desktop' / 'windows-registration-mode.json'
        record = json.loads(path.read_text(encoding='utf-8'))
        # The actual passive RunEx completed, but simulate an overrun before
        # its reply/completed metadata save. Keep the REAL rollback receipt,
        # UUID and started GUID. No manual reset follows: the supported cancel
        # must resolve this record after authority has moved on.
        record['completed'] = background['request_id']
        record['pending'] = dict(request_id=rollback['request_id'], desired='passive',
                                 phase='ready' if later_revoke else 'start_offered', original=background['instance'],
                                 started=rollback['instance']['guid'])
        path.write_text(json.dumps(record), encoding='utf-8')
        if later_revoke:
            # A separately explicit fixture revoke supersedes the old receipt.
            # Cancellation itself must neither send it nor require a new grant.
            receipt = self.cli('desktop', 'revoke', '--json', '--request-id',
                               str(uuid.uuid4()), '--reason', 'rollback')
            self.assertTrue(receipt['accepted'], receipt)
            self.assertTrue(receipt['durable'], receipt)
            self.assertTrue(receipt['checkpointed'], receipt)
        else:
            self.grant('ownership_rearm')
        ownership = self.cli('desktop', 'ownership-status', '--json')
        self.assertEqual(ownership['state'], 'revoked' if later_revoke else 'service')

        def journal():
            with closing(sqlite3.connect((self.state / 'desktop' / 'state.sqlite3').as_uri()
                                         + '?mode=ro', uri=True)) as db:
                receipt = db.execute('SELECT * FROM desktop_commands WHERE request_id=?',
                                     (rollback['request_id'],)).fetchone()
                self.assertIsNotNone(receipt)
                count = db.execute('SELECT count(*) FROM desktop_commands').fetchone()[0]
                return receipt, count

        original_receipt = journal()
        before = self.observe()
        denied = self.mode('background')
        self.assertFalse(denied['ok'], denied)
        self.assertEqual(denied['unknown']['code'], 'different_explicit_choice')
        settlement = self.mode('passive')
        self.assertFalse(settlement['ok'], settlement)
        self.assertEqual(settlement['unknown']['code'], 'original_completion_unverified')
        pending = path.read_bytes()
        wrong = self.cancel(str(uuid.uuid4()))
        self.assertFalse(wrong['ok'], wrong)
        self.assertEqual(path.read_bytes(), pending)
        cancelled = self.cancel(rollback['request_id'])
        self.assertTrue(cancelled['ok'], cancelled)
        self.assertEqual(cancelled['code'], 'observed_start_cancelled')
        self.assertTrue(cancelled['metadata_changed'])
        self.assertFalse(cancelled['manager_changed'])
        self.assertFalse(cancelled['authority_changed'])
        self.assertEqual(cancelled['observed_instance'], rollback['instance'])
        self.assertEqual(cancelled['cancelled_request_id'], rollback['request_id'])
        saved = json.loads(path.read_text(encoding='utf-8'))
        self.assertIsNone(saved['pending'])
        self.assertEqual(saved['cancelled'], rollback['request_id'])
        self.assertEqual(saved['completed'], background['request_id'])
        self.assertEqual(self.cli('desktop', 'ownership-status', '--json'), ownership)
        self.assertEqual(journal(), original_receipt)
        self.assertEqual(self.observe(), before)
        self.assertEqual(self.cli('desktop', 'status', '--json')['lifecycle']['pid'], rollback['instance']['pid'])

    def powershell(self, script, data):
        # Fixture-only native COM observation/cleanup. No task-name-wide Stop,
        # schtasks /End, PID kill or executable-pattern cleanup is used.
        request = self.root / ('sdk-' + str(uuid.uuid4()) + '.json')
        request.write_text(json.dumps(data), encoding='utf-8')
        escaped = str(request).replace("'", "''")
        lookup = r'''
function Get-OwnInstances($scheduler,$registered) {
 $path=[string]$registered.Path;
 if($path-cne ('\'+[string]$x.task)) {throw 'fixture registered task path changed'};
 $clock=[Diagnostics.Stopwatch]::StartNew();$all=$scheduler.GetRunningTasks(1);$count=$all.Count;
 if($count-lt 0 -or $count-gt 256) {throw 'fixture running inventory exceeds its bound'};
 $items=@();for($index=1;$index-le $count;$index++) {
  if($clock.ElapsedMilliseconds-ge 2000) {throw 'fixture original scan deadline expired'};
  $candidate=$all.Item($index);$candidatePath=[string]$candidate.Path;
  if([string]::IsNullOrEmpty($candidatePath)-or $candidatePath.Length-gt 32768) {throw 'fixture task path unavailable'};
  if($candidatePath-ceq $path) {
   $items+=@($candidate);if($items.Count-gt 1) {throw 'fixture has multiple exact task instances'}
  }
 };
 if($clock.ElapsedMilliseconds-ge 2000) {throw 'fixture original scan deadline expired'};
 return $items
};
'''
        body = "$ErrorActionPreference='Stop';$x=Get-Content -Raw -LiteralPath '" + escaped + "'|ConvertFrom-Json;" + lookup + script
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
$items=@();foreach($i in @(Get-OwnInstances $s $t)) {
 $p=$null;try {$p=[Diagnostics.Process]::GetProcessById($i.EnginePID).ProcessName} catch {}
 $items+=@{guid=$i.InstanceGuid;engine_pid=$i.EnginePID;engine_image=$p;state=[int]$i.State}
};@{instances=$items;registered_instances_count=$t.GetInstances(0).Count;hidden=[bool]$t.Definition.Settings.Hidden;state=[int]$t.State;last_task_result=[int64]$t.LastTaskResult;
last_run_utc=$t.LastRunTime.ToUniversalTime().ToString('o')}|ConvertTo-Json -Compress -Depth 4
''', dict(task=self.task))

    def task_history(self, started):
        # Read only this fixture's task and retain events for the recorded
        # original GUID. A disabled/unavailable history is explicit data,
        # never permission to rerun a task or infer its process has drained.
        return self.powershell(r'''
$task='\'+$x.task;
$xpath="*[EventData[Data[@Name='TaskName']='"+$task+"']]";
try {$events=@(Get-WinEvent -LogName 'Microsoft-Windows-TaskScheduler/Operational' -FilterXPath $xpath -MaxEvents 32 -ErrorAction Stop)}
catch {@{available=$false;error=$_.Exception.GetType().Name;events=@()}|ConvertTo-Json -Compress;exit 0};
$items=@();foreach($e in $events) {
 $xml=[xml]$e.ToXml();$data=@{};foreach($d in $xml.Event.EventData.Data) {
  if($d.Name-in @('InstanceId','TaskInstanceId','ResultCode','ReturnCode','ErrorCode','ErrorValue','ProcessID','ProcessId','EnginePID')) {
   $text=[string]$d.'#text';if($text.Length-gt 64) {$text=$text.Substring(0,64)};$data[[string]$d.Name]=$text
  }
 };
 $matches=$false;foreach($key in @('InstanceId','TaskInstanceId')) {
  try {if([Guid]$data[$key]-eq [Guid]$x.started) {$matches=$true}} catch {}
 };
 if($matches) {
  $items+=@{id=[int]$e.Id;record=[int64]$e.RecordId;data=$data};if($items.Count-ge 8) {break}
 }
};@{available=$true;events=$items}|ConvertTo-Json -Compress -Depth 5
''', dict(task=self.task, started=started))

    def startup_security(self):
        # Native metadata of four fixed fixture-owned entries only. This
        # explains a refused privacy check; it is never an admission result.
        return self.powershell(r'''
$items=@();foreach($p in $x.paths) {
 try {
  $acl=Get-Acl -LiteralPath $p.path -ErrorAction Stop;
  $rules=@($acl.GetAccessRules($true,$true,[Security.Principal.SecurityIdentifier]));
  $entries=@();foreach($r in ($rules|Select-Object -First 8)) {
   $sid=[string]$r.IdentityReference.Value;if($sid.Length-gt 64) {$sid=$sid.Substring(0,64)};
   $entries+=@{sid=$sid;type=[string]$r.AccessControlType;
     propagation=[string]$r.PropagationFlags;inherited=[bool]$r.IsInherited}
  };
  $items+=@{name=$p.name;owner=$acl.GetOwner([Security.Principal.SecurityIdentifier]).Value;
    protected=[bool]$acl.AreAccessRulesProtected;count=$rules.Count;entries=$entries}
 } catch {$items+=@{name=$p.name;error=$_.Exception.GetType().Name;code=[int]$_.Exception.HResult}}
};@{objects=$items}|ConvertTo-Json -Compress -Depth 5
''', dict(paths=[dict(name=name, path=str(path)) for name, path in (
            ('state', self.state), ('desktop', self.state / 'desktop'),
            ('owner', self.state / 'desktop' / 'owner.lock'),
            ('diagnostic', self.state / 'desktop' / 'startup-failure.json'))]))

    def assert_exact_user_created_state(self):
        # Check objects actually created by the scheduled service and SQLite,
        # rather than inferring ownership from a successful manager reply.
        observed = self.powershell(r'''
$items=@();foreach($p in $x.paths) {
 $acl=Get-Acl -LiteralPath $p.path -ErrorAction Stop;
 $items+=@{name=$p.name;owner=$acl.GetOwner([Security.Principal.SecurityIdentifier]).Value}
};@{objects=$items}|ConvertTo-Json -Compress -Depth 3
''', dict(paths=[dict(name=name, path=str(self.state / 'desktop' / name))
                  for name in ('owner.lock', 'state.sqlite3', 'state.sqlite3-wal', 'state.sqlite3-shm')]))
        self.assertEqual(len(observed['objects']), 4, observed)
        for item in observed['objects']:
            self.assertEqual(item['owner'], self.sid, item)

    def stop_exact_passive(self, expected, *, wrong_birth=False):
        return self.powershell(r'''
$s=New-Object -ComObject Schedule.Service;$s.Connect();$t=$s.GetFolder('\').GetTask($x.task);
$d=$t.Definition;
if($d.Actions.Count-ne 1 -or $d.Actions.Item(1).Path-cne $x.image -or $d.Principal.UserId-ne $x.sid) {throw 'fixture task definition changed'};
$values=@(Get-OwnInstances $s $t);if($values.Count-ne 1) {throw 'fixture task instance count changed'};
$i=$values[0];$i.Refresh();$guid=$i.InstanceGuid;if($guid-cne $x.guid -or $i.EnginePID-ne $x.pid) {throw 'fixture original instance changed'};
$p=[Diagnostics.Process]::GetProcessById($i.EnginePID);$handle=$p.Handle;
if($p.HasExited -or $p.StartTime.ToFileTimeUtc()-ne $x.birth -or $p.MainModule.FileName-cne $x.image) {
 @{stopped=$false;reason='original_birth_or_image_changed'}|ConvertTo-Json -Compress;exit 0
};
$i.Refresh();if($i.InstanceGuid-cne $x.guid -or $i.EnginePID-ne $p.Id) {throw 'fixture instance changed before stop'};
$i.Stop();if(-not $p.WaitForExit(5000)) {throw 'fixture original process exit unconfirmed'};
if(@(Get-OwnInstances $s $t).Count-ne 0) {throw 'fixture instance remains'};
@{stopped=$true;original_pid=$p.Id;original_guid=$guid}|ConvertTo-Json -Compress
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
            self.assert_exact_user_created_state()
            discovery = self.observe()
            self.assertTrue(discovery['hidden'], discovery)
            self.assertEqual([item['guid'] for item in discovery['instances']],
                             [passive['instance']['guid']], discovery)
            print('MODE_TASK_HIDDEN_DISCOVERY ' + json.dumps(discovery), file=sys.stderr)
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
            self.assert_exact_user_created_state()
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
            self.assert_exact_user_created_state()
            self.assertEqual(self.cli('desktop', 'ownership-status', '--json')['state'], 'revoked')
            status = self.cli('desktop', 'registration', 'status', '--json')
            self.assertEqual(status['selected_mode'], 'passive')
            self.assertEqual(status['running_mode'], 'passive')
            self.assertIsNone(status['pending_transition'])
            self.assertEqual(status['completed_request_id'], rollback['request_id'])
            self.cancel_ready_rollback_after_authority_moves(background, rollback)
            # A separately explicit new choice is admitted after cancellation;
            # the original manager effects and revoke are never replayed.
            next_background = self.mode('background')
            self.assertTrue(next_background['ok'], next_background)
            self.assertNotEqual(next_background['request_id'], rollback['request_id'])
            self.latest = next_background['instance']
            final_passive = self.mode('passive')
            self.assertTrue(final_passive['ok'], final_passive)
            self.latest = final_passive['instance']
            self.cancel_ready_rollback_after_authority_moves(next_background, final_passive, later_revoke=True)
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
                        # Fixed known leaves only: this read-only footprint
                        # narrows a guard failure even if it cannot log safely.
                        print('MODE_TASK_STARTUP_FILES ' + json.dumps({name: (
                            (self.state / 'desktop' / name).stat().st_size
                            if (self.state / 'desktop' / name).is_file() else None)
                            for name in ('owner.lock', 'state.sqlite3', 'state.sqlite3-wal',
                                         'state.sqlite3-shm', 'startup-failure.json')}), file=sys.stderr)
                        diagnostic = self.state / 'desktop' / 'startup-failure.json'
                        if diagnostic.exists():
                            self.assertLessEqual(diagnostic.stat().st_size, 4096)
                            data = json.loads(diagnostic.read_bytes())
                            self.assertEqual(data['abi'], 'desktop-startup-failure-v1')
                            self.assertEqual(data['configuration'], status['configuration'])
                            print('MODE_TASK_STARTUP_REFUSAL ' + json.dumps(data), file=sys.stderr)
                        try:
                            print('MODE_TASK_STARTUP_SECURITY ' + json.dumps(self.startup_security()), file=sys.stderr)
                        except Exception as error:
                            print('MODE_TASK_STARTUP_SECURITY_UNAVAILABLE ' + type(error).__name__, file=sys.stderr)
                        record = json.loads((self.state / 'desktop' / 'windows-registration-mode.json')
                                            .read_text(encoding='utf-8'))
                        started = (record.get('pending') or {}).get('started')
                        if started:
                            print('MODE_TASK_ORIGINAL_HISTORY ' + json.dumps(self.task_history(started)),
                                  file=sys.stderr)
                    except Exception as error:
                        print('MODE_TASK_OBSERVATION_UNAVAILABLE ' + type(error).__name__, file=sys.stderr)
                print('MODE_TASK_PRIVATE_ROOT_RETAINED ' + str(self.root), file=sys.stderr)


if __name__ == '__main__':
    unittest.main()
