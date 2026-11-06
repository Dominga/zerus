"""Real Unix frontend-request admission and idle activation reply, no native SDK.

The private native fixture owns the selected image/canonical socket and original
CLI child. It supplies cached test evidence; Root still checks kernel peers,
parent/birth/image, currentness, ancestry, order and the actual receiver path.
Run positive admission outside an agent ancestry, as required by the product.
"""
import hashlib
import json
import os
from pathlib import Path
import select
import shutil
import subprocess
import sys
import time
import unittest
import uuid

import test_desktop as kernel


@unittest.skipUnless(sys.platform.startswith(('linux', 'darwin')), 'Unix native frontend')
class Activation(unittest.TestCase):
    cli = kernel.Desktop.cli
    serve = kernel.Desktop.serve
    stop = kernel.Desktop.stop
    def setUp(self):
        kernel.Desktop.setUp(self)
        self.env['HGS_PEERS'] = ''
        self.frontend = self.root / '.local/bin/hgs-tray'
        source = Path(__file__).parent / 'fixtures/desktop_frontend.c'
        compiled = subprocess.run(['cc', '-std=c11', '-Wall', '-Wextra', '-Werror',
                                   str(source), '-o', str(self.frontend)],
                                  capture_output=True, text=True, timeout=30)
        self.assertEqual(compiled.returncode, 0, compiled.stderr)
        self.frontend.chmod(0o700)

    def begin(self, operation='activation_wait', wrong_birth=False, marked=False, task_reconcile=False, sealed=None, wrong_config=False, wrong_attempt=False, wrong_frontend=False, termination=False, missing_reference=False, rendezvous=False):
        if sealed is not None:
            # Hosted debuginfo can exceed the 256 MiB image bound. A private
            # stripped CLI normally supplies both service and relay; never raise
            # the production cap or change the installed CLI.
            self.hgs = self.root / 'native-hgs'
            shutil.copyfile(kernel.HGS, self.hgs)
            self.hgs.chmod(0o700)
            stripped = subprocess.run(['strip', '--strip-debug', str(self.hgs)], capture_output=True, timeout=30)
            self.assertEqual(stripped.returncode, 0, stripped.stderr)
            self.assertLess(self.hgs.stat().st_size, 256 << 20)
        relay_hgs = getattr(self, 'hgs', kernel.HGS)
        if missing_reference:
            self.assertIsNotNone(sealed)
            # The ACTUAL service image exceeds the unchanged cap: no startup
            # reference can be published. ELF permits an unused file tail;
            # sparse padding makes this deterministic even for small CI builds.
            self.hgs = self.root / 'unstripped-service-hgs'
            shutil.copyfile(kernel.HGS, self.hgs)
            self.hgs.chmod(0o700)
            if self.hgs.stat().st_size <= 256 << 20:
                with self.hgs.open('r+b') as image:
                    image.truncate((256 << 20) + 1)
        self.service, status = self.serve(background=True)
        attempt, request_id = str(uuid.uuid4()), str(uuid.uuid4())
        request = dict(op=operation, request_id=request_id)
        if task_reconcile:
            fixture=json.loads((Path(__file__).resolve().parents[1] / 'client/fixtures/desktop-operation-port-v1.json').read_text())
            record=json.loads(fixture['raw_record'])
            record['intent']['retained_input']=fixture['retained_input']
            raw=json.dumps(record,ensure_ascii=False,sort_keys=True,separators=(',', ':'))
            binding=fixture['binding']
            binding['key']['configuration']=status['configuration']
            binding['source_record_sha256']=hashlib.sha256(raw.encode()).hexdigest()
            binding['ui_snapshot_sha256']=hashlib.sha256(raw.encode()).hexdigest()
            intent=dict(scope=dict(configuration=status['configuration'],service_epoch=status['epoch']),
                        reconcile_id=request_id,binding=binding,original=raw)
            request=dict(op='task_start_reconcile',intent=intent)
        if termination:
            capture = dict(target=dict(computer='', name='codex/example/source', run_id='run', conversation_id='conversation'),
                           machine=str(uuid.uuid4()), root='/srv/example/work', common_dir='/srv/example/main/.git', owner_id='original-owner')
            original = dict(request_id=request_id, action='terminate', expected_run_id='run', expected_conversation_id='conversation')
            request = dict(op='termination_outcome', intent=dict(
                scope=dict(configuration=status['configuration'], service_epoch=status['epoch']),
                confirmation_id=str(uuid.uuid4()), choice='remove_after_terminate', request_id=request_id,
                capture=capture, original=json.dumps(original, separators=(',', ':'))))
        envelope = dict(abi='desktop-ui-request-v1', attempt=attempt, request=request)
        env = dict(self.env)
        if marked:
            env['HGS_RUN_ID'] = 'private-native-frontend-parent'
        selected_frontend = self.frontend
        if wrong_frontend:
            selected_frontend = self.root / 'unselected-frontend'
            shutil.copyfile(self.frontend, selected_frontend)
            selected_frontend.chmod(0o700)
        frontend = subprocess.Popen([str(selected_frontend), str(self.root / 'tmp/hgs-tray'), str(relay_hgs)] + (['rendezvous'] if rendezvous else ([sealed] if sealed else [])),
                                    env=env, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                    stderr=subprocess.PIPE, text=True)
        self.addCleanup(self.finish_child, frontend)
        self.assertTrue(select.select([frontend.stdout], [], [], 5)[0], 'native child setup bound')
        native = json.loads(frontend.stdout.readline(512))
        if sealed is not None:
            until = time.monotonic() + 5
            while True:
                try:
                    image = os.readlink(f"/proc/{native['pid']}/exe")
                except FileNotFoundError:
                    self.fail('original relay exited before its sealed image could be observed')
                if image.startswith('/memfd:zerus-helper-image'):
                    break
                self.assertLess(time.monotonic(), until, 'original child must really execute memfd')
                time.sleep(.01)
        hello = dict(abi='desktop-delegation-v1' , version=1, nonce=attempt,
                     application='com.hgdev.hgs-tray', gui='tauri',
                     build=dict(version='1.0.0', source_commit='a' * 40, source_dirty=False),
                     instance_epoch=str(uuid.uuid4()), writer_epoch=None,
                     configuration=status['configuration'], live=True, replay=False)
        cached = dict(attempt=attempt, request_id=request_id,
                      fingerprint=hashlib.sha256(json.dumps(request, ensure_ascii=False, separators=(',', ':')).encode()).hexdigest(),
                      child_pid=native['pid'], child_birth='replaced' if wrong_birth else native['birth'], serial=1)
        if wrong_config:
            hello['configuration'] = 'b' * 64
        if wrong_attempt:
            cached['attempt'] = str(uuid.uuid4())
        if task_reconcile or termination:
            cached['ownership_mode']='service'
        for value in (envelope, dict(hello=hello, request=cached)):
            frontend.stdin.write(json.dumps(value, ensure_ascii=False, separators=(',', ':')) + '\n')
        frontend.stdin.write(f'desktop-ui-request-v1 {attempt}\n')
        frontend.stdin.flush()
        return frontend, request_id, status

    @staticmethod
    def finish_child(frontend):
        if frontend.poll() is None:
            frontend.terminate()
        if frontend.stdin is not None:
            frontend.stdin.close()
            frontend.stdin = None
        frontend.communicate(timeout=8)

    def result(self, frontend):
        frontend.stdin.close()
        frontend.stdin = None
        output, error = frontend.communicate(timeout=25)
        self.assertEqual(frontend.returncode, 0, output + error)
        self.assertIn('original_reaped=1', error)
        return json.loads(output), error

    def test_authenticated_activation_read_reaches_real_receiver_and_returns_idle_reply(self):
        if sys.platform == 'darwin':
            import macos_ancestry
            topology = macos_ancestry.topology()
            print('ACTIVATION_OBSERVED_CHAIN ' + json.dumps(topology), file=sys.stderr)
            unsupported = macos_ancestry.unsupported_hosted_foreign(topology, os.getuid(), os.environ)
            if unsupported is not None:
                self.skipTest('observed unsupported hosted foreign ancestor: ' + json.dumps(unsupported))
        frontend, request, initial = self.begin()
        until = time.monotonic() + 8
        waiting = False
        while time.monotonic() < until and frontend.poll() is None:
            status = self.cli('desktop', 'status', '--json')
            waiting = status.get('activation_transport', {}).get('waiting', False)
            if waiting:
                break
            time.sleep(.02)
        reply, error = self.result(frontend)
        self.assertTrue(waiting, reply)
        self.assertEqual(reply, dict(abi='desktop-activation-v1', request_id=request,
                                    configuration=initial['configuration'],
                                    service_epoch=initial['epoch'], activation=None))
        self.assertIn('proofs=1 ', error)
        status = self.cli('desktop', 'status', '--json')
        self.assertFalse(status['activation_transport']['waiting'])
        self.assertFalse(status['automatic_approvals'])
        self.assertEqual(status['writers_enabled'], initial['writers_enabled'])
        self.assertEqual(status['desktop_ownership']['revision'], 0)

    def test_original_rendezvous_keeps_the_closed_platform_command_and_retention(self):
        if sys.platform == 'darwin':
            import macos_ancestry
            chain = macos_ancestry.topology()
            unsupported = macos_ancestry.unsupported_hosted_foreign(chain, os.getuid(), os.environ)
            if unsupported is not None:
                self.skipTest('observed unsupported hosted foreign ancestor: ' + json.dumps(unsupported))
        frontend, request, service = self.begin(termination=True, rendezvous=True)
        reply, error = self.result(frontend)
        if sys.platform != 'darwin':
            # Linux still accepts ONLY the original exact argv. The shared
            # transport entry is not new native relay permission there.
            # The nested termination UUID is not decoded/promoted before
            # authentication; this exact refusal intentionally has no ID.
            self.assertEqual(reply, dict(request_id=None, accepted=False, durable=False,
                                         code='frontend_authentication_refused'))
            self.assertIn('proofs=0 original_reaped=1', error)
            return
        self.assertTrue(reply['accepted'], reply)
        self.assertTrue(reply['durable'], reply)
        self.assertFalse(reply['cleanup_authorized'])
        self.assertIn('proofs=1 original_reaped=1', error)
        from contextlib import closing
        import sqlite3
        with closing(sqlite3.connect(self.desktop / 'state.sqlite3')) as db:
            rows = db.execute('SELECT request_id,original FROM desktop_termination_outcomes').fetchall()
        self.assertEqual(len(rows), 1)
        self.assertEqual(rows[0][0], request)
        original = json.loads(rows[0][1])
        self.assertEqual(original['scope'], dict(configuration=service['configuration'], service_epoch=service['epoch']))
        self.assertEqual(json.loads(original['original'])['request_id'], request)

    @unittest.skipUnless(sys.platform == 'linux', 'Linux kernel sealed relay')
    def test_sealed_original_relay_reaches_the_real_native_activation_receiver(self):
        frontend, request, service = self.begin(sealed='sealed')
        reply, error = self.result(frontend)
        self.assertEqual(reply, dict(abi='desktop-activation-v1', request_id=request,
                                    configuration=service['configuration'], service_epoch=service['epoch'], activation=None))
        self.assertIn('proofs=1 ', error)

    @unittest.skipUnless(sys.platform == 'linux', 'Linux kernel sealed relay')
    def test_sealed_original_retains_only_its_exact_termination_data_at_the_typed_actor(self):
        from contextlib import closing
        import sqlite3
        frontend, request, service = self.begin(sealed='sealed', termination=True)
        reply, error = self.result(frontend)
        self.assertTrue(reply['accepted'], reply)
        self.assertTrue(reply['durable'], reply)
        self.assertFalse(reply['cleanup_authorized'])
        self.assertIn('proofs=1 ', error)
        with closing(sqlite3.connect(self.desktop / 'state.sqlite3')) as db:
            rows = db.execute('SELECT request_id,original FROM desktop_termination_outcomes').fetchall()
            self.assertEqual(len(rows), 1)
            self.assertEqual(rows[0][0], request)
            original = json.loads(rows[0][1])
            self.assertEqual(original['scope'], dict(configuration=service['configuration'], service_epoch=service['epoch']))
            self.assertEqual(json.loads(original['original'])['request_id'], request)

    @unittest.skipUnless(sys.platform == 'linux', 'Linux kernel sealed relay')
    def test_missing_startup_reference_refuses_a_real_sealed_relay_before_native_proof(self):
        frontend, request, _ = self.begin(sealed='sealed', missing_reference=True)
        self.assertGreater(Path(f'/proc/{self.service.pid}/exe').stat().st_size, 256 << 20)
        reply, error = self.result(frontend)
        self.assertEqual(reply, dict(request_id=request, accepted=False, durable=False,
                                    code='frontend_authentication_refused'))
        # This fails even under agent ancestry if the image check is skipped:
        # canonical proof precedes the independent ancestry refusal.
        self.assertIn('proofs=0 original_reaped=1', error)
        self.assertFalse(self.cli('desktop', 'status', '--json')['activation_transport']['waiting'])

    @unittest.skipUnless(sys.platform == 'linux', 'Linux kernel sealed relay')
    def test_sealed_equivalence_never_bypasses_original_native_proof_or_ancestry(self):
        for changed in ('wrong_birth', 'wrong_config', 'wrong_attempt', 'wrong_frontend', 'marked', 'wrong-parent'):
            with self.subTest(changed=changed):
                # A separate private service/reference for every original.
                case = Activation()
                case.setUp()
                try:
                    frontend, request, _ = case.begin(sealed=changed if changed.startswith('wrong-') else 'sealed', **({changed: True} if not changed.startswith('wrong-') else {}))
                    reply, _ = case.result(frontend)
                    self.assertEqual(reply, dict(request_id=request, accepted=False, durable=False,
                                                 code='frontend_authentication_refused'))
                    self.assertFalse(case.cli('desktop', 'status', '--json')['activation_transport']['waiting'])
                finally:
                    case.doCleanups()

    @unittest.skipUnless(sys.platform == 'linux', 'Linux kernel sealed relay')
    def test_every_incomplete_seal_set_refuses_before_canonical_native_proof(self):
        for mode in ('missing-write', 'missing-grow', 'missing-shrink', 'missing-seal', 'future-write'):
            with self.subTest(mode=mode):
                case = Activation()
                case.setUp()
                try:
                    frontend, request, _ = case.begin(sealed=mode)
                    reply, error = case.result(frontend)
                    self.assertEqual(reply, dict(request_id=request, accepted=False, durable=False,
                                                 code='frontend_authentication_refused'))
                    self.assertIn('proofs=0 ', error)
                finally:
                    case.doCleanups()

    def test_wrong_original_child_birth_refuses_before_receiver(self):
        frontend, request, _ = self.begin(wrong_birth=True)
        reply, error = self.result(frontend)
        self.assertEqual(reply, dict(request_id=request, accepted=False, durable=False,
                                    code='frontend_authentication_refused'))
        self.assertIn('proofs=1 ', error)
        self.assertFalse(self.cli('desktop', 'status', '--json')['activation_transport']['waiting'])

    def test_authenticated_task_reconciliation_without_its_current_lease_is_refused(self):
        """Native request reaches its closed kind, but cached Service grants no effect."""
        from contextlib import closing
        import sqlite3
        frontend,request,_=self.begin(task_reconcile=True)
        reply,error=self.result(frontend)
        self.assertEqual(reply,dict(request_id=request,accepted=False,durable=False,
                                   code='task_start_reconcile_unavailable'))
        self.assertIn('proofs=1 ',error)
        with closing(sqlite3.connect(self.desktop / 'state.sqlite3')) as db:
            for table in ('desktop_operations','desktop_operation_attempts','desktop_operation_custody'):
                self.assertEqual(db.execute('SELECT count(*) FROM '+table).fetchone()[0],0)

    def test_marked_native_frontend_cannot_admit_activation_reader(self):
        frontend, request, _ = self.begin(marked=True)
        reply, _ = self.result(frontend)
        self.assertEqual(reply, dict(request_id=request, accepted=False, durable=False,
                                    code='frontend_authentication_refused'))
        self.assertFalse(self.cli('desktop', 'status', '--json')['activation_transport']['waiting'])

    def test_unknown_operation_refuses_before_native_proof_or_receiver(self):
        frontend, request, _ = self.begin(operation='unknown_operation')
        reply, error = self.result(frontend)
        self.assertEqual(reply, dict(request_id=request, accepted=False, durable=False,
                                    code='frontend_authentication_refused'))
        self.assertIn('proofs=0 ', error)
        self.assertFalse(self.cli('desktop', 'status', '--json')['activation_transport']['waiting'])


if __name__ == '__main__':
    unittest.main()
