"""Unregistered resident hosts cannot use a launcher's caller identity.

Actual official DeepSeek with the maintained keyless model. Only private
native/config/state homes and fixture-owned processes; never a resident user
host. A legacy adapter is emulated inside the private host without changing
tracked bridge source. This is caller refusal, not host adoption or restart.
"""
import hashlib
import json
import os
import subprocess
import time
import unittest
import uuid

from test_dsh_shell_identity import DirectlyStarted, HostStartedByHgs, Hosted, DSH, HGS, fixture

REFUSED = 'unregistered DeepSeek agent host'


@unittest.skipUnless(fixture.required() or DSH and HGS.exists(), 'official dsh and built hgs required')
class UnregisteredLegacyHost(DirectlyStarted, unittest.TestCase):
    LEGACY_BRIDGE = True

    @classmethod
    def stop_host(cls):
        super().stop_host()
        if getattr(cls, 'proc', None):
            # Reap this exact original child before removing its private home.
            cls.proc.wait(timeout=15)

    def test_task_mutations_and_every_send_form_refuse_without_host_repair(self):
        self.assertFalse((self.host_dir / 'host.identity.json').exists())
        self.assertFalse((self.host_dir / 'host.pid').exists())
        original_pid = self.proc.pid
        ref, target = self.task(), self.launch()
        stripped = {key: None for key in ('HGS_AGENT', 'HGS_SESSION', 'HGS_RUN_ID',
                                         'HGS_SESSION_ID', 'HGS_ACCOUNT_ID', 'HGS_DSH_CALL_TOKEN')}
        forms = [('send', [str(HGS), 'send', target['name'], '--json']),
                 ('send-now', [str(HGS), 'send-now', target['name'], '--json']),
                 ('state-send', [str(HGS), '__state', 'send', target['name'], '--json']),
                 ('peer-send', [str(HGS), '--dry-run', '@peer', 'send', target['name'], '--json'])]
        runs = []
        for label, changes in (('inherited', {}), ('stripped', stripped)):
            runs.append((label + '-task', [str(HGS), 'task', 'comment', ref], 'wrong caller', changes))
            runs.append((label + '-peer-task', [str(HGS), '--dry-run', '@peer', 'task', 'comment', ref],
                         'wrong remote caller', dict(changes, HGS_PEERS='peer')))
            for name, argv in forms:
                request = json.dumps(dict(request_id=str(uuid.uuid4()), expected_run_id=target['run_id'],
                                          expected_conversation_id=target['conversation_id'],
                                          text='UNREGISTERED_HOST_MUST_NOT_DELIVER'))
                runs.append((label + '-' + name, argv, request, dict(changes, HGS_PEERS='peer')))
        result = self.shell(self.session, runs=runs)
        for name, answer in result['runs'].items():
            self.assertNotEqual(answer['code'], 0, (name, answer))
            self.assertIn(REFUSED, answer['stderr'], (name, answer))
            self.assertIn('new host', answer['stderr'], (name, answer))
            self.assertNotIn('ssh', answer['stdout'])
        history = json.loads(self.call('task', 'get', ref, '--history-limit', '20').stdout)['task']['log']
        self.assertEqual([entry['kind'] for entry in history], ['created'])
        inspected = self.call('inspect', target['name']).stdout
        self.assertNotIn('UNREGISTERED_HOST_MUST_NOT_DELIVER', inspected)
        self.assertEqual(self.proc.pid, original_pid)
        self.assertIsNone(self.proc.poll(), 'refusal must leave the original host running')
        self.assertFalse((self.host_dir / 'host.identity.json').exists(), 'refusal cannot adopt the host')
        self.assertFalse((self.host_dir / 'host.pid').exists())
        self.call('task', 'comment', ref, input='person outside the host is unchanged')
        if self.launcher['pid'] == os.getpid():
            self.call('task', 'comment', ref, env=self.launcher_env,
                      input='original agent outside the host is unchanged')
            history = json.loads(self.call('task', 'get', ref, '--history-limit', '20').stdout)['task']['log']
            self.assertEqual(history[-1]['actor']['session'], self.launcher['name'])

    def test_guarded_private_sends_use_the_same_native_caller_refusal(self):
        target = self.launch()
        machine = json.loads(self.call('swarm', 'get').stdout)['node_id']
        forms = []
        for verb in ('send', 'send-now'):
            for client in ([], ['--client', 'private-peer']):
                request = dict(request_id=str(uuid.uuid4()), expected_run_id=target['run_id'],
                               expected_conversation_id=target['conversation_id'], text='GUARDED_MUST_NOT_DELIVER')
                forms.append((verb + str(len(client)), [str(HGS), '--expect-machine', machine,
                             *client, '__state', verb, target['name'], '--json'], json.dumps(request)))
        result = self.shell(self.session, runs=forms)
        for name, answer in result['runs'].items():
            self.assertNotEqual(answer['code'], 0, (name, answer))
            self.assertIn(REFUSED, answer['stderr'], (name, answer))
        self.assertNotIn('GUARDED_MUST_NOT_DELIVER', self.call('inspect', target['name']).stdout)
        request = dict(request_id=str(uuid.uuid4()), expected_run_id=target['run_id'],
                       expected_conversation_id=target['conversation_id'], text='person outside the host')
        self.call('--expect-machine', machine, '__state', 'send', target['name'], '--json',
                  input=json.dumps(request))

    def test_an_unrelated_damaged_record_never_masks_an_observable_host(self):
        target = self.launch()
        folder = self.host_dir.parent / ('f' * 16)
        folder.mkdir()
        path = folder / 'host.identity.json'
        path.write_text('{}')
        try:
            request = dict(request_id=str(uuid.uuid4()), expected_run_id=target['run_id'],
                           expected_conversation_id=target['conversation_id'], text='DAMAGED_RECORD_MUST_NOT_DELIVER')
            result = self.shell(self.session, runs=[('send', [str(HGS), 'send', target['name'], '--json'],
                                                    json.dumps(request))])['runs']['send']
            self.assertNotEqual(result['code'], 0, result)
            self.assertIn(REFUSED, result['stderr'], result)
        finally:
            path.unlink()
            folder.rmdir()
        self.assertNotIn('DAMAGED_RECORD_MUST_NOT_DELIVER', self.call('inspect', target['name']).stdout)


class UnregisteredHostBelowItsLauncher(UnregisteredLegacyHost):
    @classmethod
    def prepare(cls):
        super().prepare()
        started = subprocess.check_output(['ps', '-p', str(os.getpid()), '-o', 'lstart='],
                                          env={'LC_ALL': 'C', 'TZ': 'UTC', 'PATH': '/usr/bin:/bin'},
                                          text=True).strip()
        # The actual private test driver remains above its host, like a live
        # launcher. A non-ancestor stand-in must not mask wrong attribution.
        cls.launcher.update(pid=os.getpid(), process_start=started, last_event_at=time.time())
        path = cls.state / (hashlib.sha256(cls.launcher['name'].encode()).hexdigest() + '.json')
        path.write_text(json.dumps(cls.launcher))


class UnregisteredHostWithNodeOptions(UnregisteredHostBelowItsLauncher):
    NODE_OPTIONS_ARGV = ['--no-warnings']


class RegisteredHostWithDamagedRecord(Hosted, unittest.TestCase):
    setUpClass = classmethod(HostStartedByHgs.setUpClass.__func__)
    tearDownClass = classmethod(HostStartedByHgs.tearDownClass.__func__)

    def test_own_hosted_proof_is_unchanged_by_an_unrelated_bad_record(self):
        ref, target = self.task(), self.launch()
        folder = self.host_dir.parent / ('f' * 16)
        folder.mkdir()
        path = folder / 'host.identity.json'
        path.write_text('{}')
        try:
            request = dict(request_id=str(uuid.uuid4()), expected_run_id=target['run_id'],
                           expected_conversation_id=target['conversation_id'], text='own registered host')
            result = self.shell(self.first, runs=[
                ('comment', [str(HGS), 'task', 'comment', ref], 'own hosted comment'),
                ('send', [str(HGS), 'send', target['name'], '--json'], json.dumps(request))])
            for name, answer in result['runs'].items():
                self.assertEqual(answer['code'], 0, (name, answer))
        finally:
            path.unlink()
            folder.rmdir()
        history = json.loads(self.call('task', 'get', ref, '--history-limit', '20').stdout)['task']['log']
        self.assertEqual(history[-1]['actor']['session'], self.first['name'])
        self.assertIn('own registered host', self.call('inspect', target['name']).stdout)


if __name__ == '__main__':
    unittest.main()
