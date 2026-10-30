"""Passive desktop-v1 kernel through real CLI processes in private state.

No system manager, native agent, production state or tmux server is used.
"""
from contextlib import closing
import hashlib
import json
import os
from pathlib import Path
import queue
import socket
import sqlite3
import subprocess
import tempfile
import threading
import time
import unittest
import uuid

HGS = Path(os.environ.get('HGS_TEST_BIN', Path(__file__).resolve().parents[1] / 'target/debug/hgs')).resolve()


class Desktop(unittest.TestCase):
    def setUp(self):
        temp = tempfile.TemporaryDirectory(prefix='hgs-desk-')
        self.addCleanup(temp.cleanup)
        self.root = Path(temp.name).resolve()
        self.state = self.root / 'state'
        self.desktop = self.state / 'desktop'
        self.config = self.root / 'config'
        self.config.mkdir(mode=0o700)
        self.env = {k: v for k, v in os.environ.items() if not k.startswith('HGS_') and k not in ('TMUX', 'TMUX_PANE')}
        self.env.update(HOME=str(self.root), USERPROFILE=str(self.root),
                        HGS_CONFIG_DIR=str(self.config), HGS_STATE_DIR=str(self.state),
                        HGS_SELF='example', HGS_PEERS='peer.example')
        if os.name != 'nt':
            # A passive service must not contact any peer/manager/terminal.
            fake = self.root / '.local/bin'
            fake.mkdir(parents=True)
            for name in ('ssh', 'tmux', 'systemctl', 'launchctl', 'schtasks'):
                program = fake / name
                program.write_text('#!/bin/sh\nprintf called >> "$HOME/unexpected-tool"\nexit 99\n')
                program.chmod(0o700)

    def cli(self, *args, data=None, ok=True, env=None):
        result = subprocess.run([str(HGS), *args], input=None if data is None else json.dumps(data),
                                env=env or self.env, capture_output=True, text=True, timeout=10)
        if ok:
            self.assertEqual(result.returncode, 0, result.stderr)
            return json.loads(result.stdout)
        self.assertNotEqual(result.returncode, 0, result.stdout)
        return result

    def stop(self, process):
        if process.poll() is None:
            process.terminate()
            output, error = process.communicate(timeout=8)
            if os.name != 'nt':
                self.assertEqual(process.returncode, 0, error + output)

    def serve(self):
        # The service must create a private endpoint even when its launcher
        # supplies a permissive mask; probing readiness races socket creation.
        options = {'umask': 0} if os.name != 'nt' else {}
        process = subprocess.Popen([str(HGS), 'desktop', 'serve'], env=self.env,
                                   stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, **options)
        self.addCleanup(self.stop, process)
        deadline = time.monotonic() + 10
        while time.monotonic() < deadline:
            if process.poll() is not None:
                self.fail('service failed to start: ' + process.communicate()[1])
            status = self.cli('desktop', 'status', '--json')
            if status['running']:
                return process, status
            time.sleep(0.02)
        self.fail('service did not become ready')

    def failed_serve(self, env=None):
        return self.cli('desktop', 'serve', ok=False, env=env)

    def watch(self):
        process = subprocess.Popen([str(HGS), 'desktop', 'watch', '--json'], env=self.env,
                                   stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
        frames = queue.Queue(maxsize=32)

        def read():
            for line in process.stdout:
                frames.put(json.loads(line), timeout=5)

        reader = threading.Thread(target=read, daemon=True)
        reader.start()

        def close():
            if not process.stdin.closed:
                process.stdin.close()
            if process.poll() is None:
                process.wait(timeout=5)
            reader.join(timeout=5)
            self.assertFalse(reader.is_alive(), 'watch reader did not drain')
            for file in (process.stdout, process.stderr):
                file.close()

        self.addCleanup(close)
        return process, frames.get(timeout=5), frames

    def test_status_is_read_only_and_does_not_start_service(self):
        status = self.cli('desktop', 'status', '--json')
        self.assertFalse(status['running'])
        self.assertFalse(self.state.exists())
        self.cli('desktop', 'watch', '--json', ok=False)
        self.assertFalse(self.state.exists())
        self.cli('@peer.example', 'desktop', 'status', '--json', ok=False)
        self.cli('--dry-run', 'desktop', 'serve', ok=False)
        self.assertFalse((self.root / 'unexpected-tool').exists())

    @unittest.skipIf(os.name == 'nt', 'Windows preview requires an installed stable CLI')
    def test_registration_preview_preserves_selected_paths_without_installing(self):
        preview = self.cli('desktop', 'registration', '--json')
        self.assertEqual(preview['operation'], 'preview')
        expected = ['desktop', 'serve', '--config-dir', str(self.config), '--state-dir', str(self.state)]
        if os.uname().sysname == 'Darwin':
            expected.append('--launchd')
        self.assertEqual(preview['argv'][1:], expected)
        self.assertEqual(Path(preview['argv'][0]).resolve(), HGS)
        self.assertEqual(preview['configuration'], self.cli('desktop', 'status', '--json')['configuration'])
        self.assertFalse(Path(preview['destination']).exists())
        self.assertFalse(self.state.exists())
        self.assertFalse((self.root / 'unexpected-tool').exists())

    def test_launchd_permanent_refusal_stops_restarts_but_ordinary_cli_fails(self):
        args = ['desktop', 'serve', '--config-dir', 'relative', '--state-dir', str(self.state)]
        result = self.cli(*args, ok=False)
        self.assertEqual(result.returncode, 78)
        refused = subprocess.run([str(HGS), *args, '--launchd'], env=self.env, capture_output=True, text=True, timeout=10)
        self.assertEqual(refused.returncode, 0, refused.stderr)
        self.assertIn('automatic restart paused', refused.stderr)
        self.assertFalse(self.state.exists())
        # A private-dir refusal after configuration loading uses the same lane.
        self.state.mkdir()
        (self.state / 'desktop').write_text('foreign data')
        args[3] = str(self.config)
        result = self.cli(*args, ok=False)
        self.assertEqual(result.returncode, 78)
        refused = subprocess.run([str(HGS), *args, '--launchd'], env=self.env, capture_output=True, text=True, timeout=10)
        self.assertEqual(refused.returncode, 0, refused.stderr)
        self.assertIn('desktop startup refused', refused.stderr)
        self.assertEqual((self.state / 'desktop').read_text(), 'foreign data')
        # Configuration loading fails before desktop dispatch and must not loop.
        (self.config / 'config').mkdir()
        refused = subprocess.run([str(HGS), *args, '--launchd'], env=self.env, capture_output=True, text=True, timeout=10)
        self.assertEqual(refused.returncode, 0, refused.stderr)
        self.assertIn('automatic restart paused', refused.stderr)
        # Unrelated/prefixed commands cannot gain the launchd exit mapping.
        self.cli('@example', *args, '--launchd', ok=False)
        self.assertFalse((self.root / 'unexpected-tool').exists())

    def test_registered_launch_pins_directories_instead_of_inherited_environment(self):
        selected = self.root / 'selected config'
        state = self.root / 'selected state'
        selected.mkdir(mode=0o700)
        process = subprocess.Popen([str(HGS), 'desktop', 'serve', '--config-dir', str(selected), '--state-dir', str(state)], env=self.env,
                                   stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
        self.addCleanup(self.stop, process)
        env = dict(self.env, HGS_CONFIG_DIR=str(selected), HGS_STATE_DIR=str(state))
        deadline = time.monotonic() + 10
        while time.monotonic() < deadline:
            self.assertIsNone(process.poll(), 'registered launch exited')
            if self.cli('desktop', 'status', '--json', env=env)['running']:
                break
            time.sleep(0.02)
        else:
            self.fail('registered launch did not use selected paths')
        self.assertFalse(self.state.exists())
        self.assertFalse(self.cli('desktop', 'status', '--json')['running'])
        self.cli('desktop', 'serve', '--config-dir', 'relative', '--state-dir', str(state), ok=False)
        self.cli('@example', 'desktop', 'serve', '--config-dir', str(selected), '--state-dir', str(state), ok=False)
        self.assertFalse(self.state.exists(), 'a prefixed invocation must never ignore registered paths')

    def test_passive_kernel_exclusion_and_restart(self):
        process, first = self.serve()
        self.assertEqual(first['ownership'], 'passive')
        self.assertFalse(first['writers_enabled'])
        self.assertFalse(first['automatic_approvals'])
        self.assertEqual(first['upstreams'], [])
        self.assertFalse(first['complete'])
        self.failed_serve()
        self.assertEqual(self.cli('desktop', 'status', '--json')['epoch'], first['epoch'])
        self.stop(process)
        _, second = self.serve()
        self.assertNotEqual(first['epoch'], second['epoch'])
        self.assertEqual(second['generation'], first['generation'] + 1)
        self.assertFalse((self.state / 'desktop-presence.json').exists())
        self.assertFalse((self.state / 'read-marks.json').exists())
        self.assertFalse((self.root / 'unexpected-tool').exists())

    def test_queries_and_effect_refusal_have_exact_request_ids(self):
        self.serve()
        request_id = str(uuid.uuid4())
        result = self.cli('desktop', 'request', '--json', data={'op': 'ping', 'request_id': request_id})
        self.assertEqual(result, dict(request_id=request_id, accepted=True, durable=False, op='pong'))
        for op in ('read', 'approval_settings', 'focus', 'activate', 'subscribe', 'unknown'):
            result = self.cli('desktop', 'request', '--json', data={'op': op, 'request_id': request_id, 'enabled': True})
            self.assertEqual(result, dict(request_id=request_id, accepted=False, code='handoff_required'))
        result = self.cli('desktop', 'request', '--json', data={'op': 'ping', 'request_id': 'not-a-uuid'})
        self.assertEqual(result['code'], 'invalid_request_id')
        self.assertFalse((self.desktop / 'approval-log').exists())

    def import_evidence(self, phase='complete', evidence='drained'):
        configuration = self.cli('desktop', 'status', '--json')['configuration']
        import_id = '11111111-1111-4111-8111-111111111111'
        folder = self.desktop / 'imports' / import_id
        folder.mkdir(parents=True, exist_ok=True, mode=0o700)
        for directory in (self.state, self.desktop, folder.parent, folder):
            directory.chmod(0o700)
        record = json.dumps(dict(version=1, configuration=configuration, source='com.hgdev.hgs-tray',
                                 import_id=import_id, evidence=evidence, created_at=1,
                                 automatic_approvals=False, sources=[], pages=[])).encode()
        record_path = folder / 'record-v1.json'
        record_path.write_bytes(record)
        record_path.chmod(0o600)
        marker = dict(version=1, configuration=configuration, source='com.hgdev.hgs-tray', import_id=import_id,
                      writer_epoch='22222222-2222-4222-8222-222222222222', phase=phase,
                      import_record=dict(version=1, sha256=hashlib.sha256(record).hexdigest()), automatic_approvals=False)
        marker_path = self.desktop / 'handoff-v1.json'
        marker_path.write_text(json.dumps(marker))
        marker_path.chmod(0o600)
        return marker_path, record_path, marker

    def assert_passive_import(self, reason):
        process, status = self.serve()
        self.assertEqual(status['migration']['reason'], reason)
        self.assertFalse(status['writers_enabled'])
        self.assertFalse(status['automatic_approvals'])
        self.assertEqual(status['upstreams'], [])
        result = self.cli('desktop', 'request', '--json', data={'op': 'read', 'request_id': str(uuid.uuid4())})
        self.assertFalse(result['accepted'])
        self.stop(process)
        self.assertFalse((self.root / 'unexpected-tool').exists())

    def test_import_phases_never_substitute_for_current_writer_proof(self):
        for phase, evidence, reason in [('revoked', 'drained', 'revoked'),
                                        ('imported', 'drained', 'handoff_incomplete'),
                                        ('quiesced', 'drained', 'handoff_incomplete'),
                                        ('complete', 'staged', 'import_not_drained'),
                                        ('complete', 'drained', 'delegation_required')]:
            with self.subTest(phase=phase, evidence=evidence):
                marker, record, _ = self.import_evidence(phase, evidence)
                before = marker.read_bytes(), record.read_bytes()
                self.assert_passive_import(reason)
                self.assertEqual((marker.read_bytes(), record.read_bytes()), before)
        # Even a second start with a complete, drained record stays passive.
        self.assert_passive_import('delegation_required')

    def test_invalid_or_newer_handoff_is_preserved_and_refused(self):
        changes = [dict(version=2), dict(phase='future'), dict(configuration='b' * 64),
                   dict(source='dev.zerus.next'), dict(automatic_approvals=True),
                   dict(import_id='../escape'), dict(unknown='field')]
        for change in changes:
            with self.subTest(change=change):
                path, record, marker = self.import_evidence()
                marker.update(change)
                path.write_text(json.dumps(marker))
                before = path.read_bytes(), record.read_bytes()
                self.assert_passive_import('invalid_import_evidence')
                self.assertEqual((path.read_bytes(), record.read_bytes()), before)
        for contents in (b'{', b' ' * (64 * 1024 + 1)):
            path, _, _ = self.import_evidence()
            path.write_bytes(contents)
            self.assert_passive_import('invalid_import_evidence')
            self.assertEqual(path.read_bytes(), contents)

    def test_import_manifest_digest_size_and_private_leaf_are_checked(self):
        for contents in (b'{}', b' ' * (256 * 1024 + 1)):
            _, record, _ = self.import_evidence()
            record.write_bytes(contents)
            self.assert_passive_import('invalid_import_evidence')
            self.assertEqual(record.read_bytes(), contents)
        _, record, _ = self.import_evidence()
        record.unlink()
        self.assert_passive_import('invalid_import_evidence')
        if os.name != 'nt':
            marker, record, _ = self.import_evidence()
            record.chmod(0o644)
            self.assert_passive_import('invalid_import_evidence')
            record.chmod(0o600)
            saved = marker.read_bytes()
            marker.unlink()
            marker.symlink_to(record)
            self.assert_passive_import('invalid_import_evidence')
            self.assertTrue(marker.is_symlink())
            marker.unlink()
            os.mkfifo(marker, 0o600)
            self.assert_passive_import('invalid_import_evidence')
            marker.unlink()
            marker.write_bytes(saved)
            marker.chmod(0o600)
            sibling = record.parent.with_name('foreign-import')
            record.parent.rename(sibling)
            record.parent.symlink_to(sibling, target_is_directory=True)
            self.assert_passive_import('invalid_import_evidence')
            self.assertTrue(record.parent.is_symlink())

    @unittest.skipIf(os.name == 'nt', 'POSIX flock import-commit contention fixture')
    def test_import_commit_lock_is_separate_and_contention_stays_passive(self):
        import fcntl
        self.import_evidence()
        lock_path = self.desktop / 'handoff.lock'
        with lock_path.open('w') as lock:
            lock_path.chmod(0o600)
            fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
            self.assert_passive_import('handoff_unavailable')
        self.assert_passive_import('delegation_required')

    def test_two_clients_share_epoch_and_eof_detaches_only_one(self):
        service, status = self.serve()
        first, a, _ = self.watch()
        second, b, _ = self.watch()
        self.assertEqual(a['epoch'], status['epoch'])
        self.assertEqual(b['epoch'], a['epoch'])
        self.assertEqual((a['t'], b['t']), ('hello', 'hello'))
        first.stdin.close()
        first.wait(timeout=5)
        self.assertEqual(first.returncode, 0, first.stderr.read())
        self.assertIsNone(second.poll())
        self.assertIsNone(service.poll())
        deadline = time.monotonic() + 5
        while time.monotonic() < deadline:
            if self.cli('desktop', 'status', '--json')['subscribers'] == 1:
                break
            time.sleep(0.02)
        else:
            self.fail('closed client retained its subscriber slot')

    def test_subscriber_limit_preserves_status_lane(self):
        self.serve()
        for _ in range(8):
            _, hello, _ = self.watch()
            self.assertEqual(hello['t'], 'hello')
        ninth, refusal, _ = self.watch()
        self.assertEqual(refusal['value']['code'], 'subscriber_limit')
        ninth.wait(timeout=5)
        self.assertEqual(self.cli('desktop', 'status', '--json')['subscribers'], 8)

    def test_watch_stays_attached_across_two_heartbeats(self):
        service, status = self.serve()
        watcher, hello, frames = self.watch()
        for sequence in (hello['seq'] + 1, hello['seq'] + 2):
            heartbeat = frames.get(timeout=13)
            self.assertEqual(heartbeat['t'], 'heartbeat')
            self.assertEqual(heartbeat['epoch'], status['epoch'])
            self.assertEqual(heartbeat['seq'], sequence)
            self.assertIsNone(watcher.poll(), watcher.stderr.read() if watcher.poll() is not None else '')
            self.assertEqual(self.cli('desktop', 'status', '--json')['subscribers'], 1)
        self.stop(service)
        watcher.wait(timeout=5)
        self.assertEqual(watcher.returncode, 0)

    def test_newer_schema_is_preserved(self):
        self.desktop.mkdir(mode=0o700, parents=True)
        self.state.chmod(0o700)
        database = self.desktop / 'state.sqlite3'
        with closing(sqlite3.connect(database)) as db, db:
            db.execute('PRAGMA user_version=999')
            db.execute('CREATE TABLE future (payload TEXT)')
            db.execute("INSERT INTO future VALUES ('preserved')")
        database.chmod(0o600)
        before = database.read_bytes()
        self.assertIn('newer hgs', self.failed_serve().stderr)
        self.assertEqual(database.read_bytes(), before)
        self.assertFalse((self.desktop / 'state.sqlite3-wal').exists())

    def test_another_configuration_cannot_reuse_the_store(self):
        process, first = self.serve()
        other = self.root / 'other-config'
        other.mkdir(mode=0o700)
        env = dict(self.env, HGS_CONFIG_DIR=str(other))
        self.cli('desktop', 'status', '--json', env=env, ok=False)
        self.stop(process)
        database = self.desktop / 'state.sqlite3'
        before = database.read_bytes()
        self.assertIn('another configuration', self.failed_serve(env=env).stderr)
        self.assertEqual(before, database.read_bytes())
        _, second = self.serve()
        self.assertEqual(second['generation'], first['generation'] + 1)

    @unittest.skipIf(os.name == 'nt', 'Unix socket and mode checks')
    def test_unsafe_files_and_socket_targets_are_preserved(self):
        self.desktop.mkdir(mode=0o700, parents=True)
        self.state.chmod(0o700)
        foreign = self.root / 'foreign'
        foreign.write_bytes(b'preserve these bytes')
        for name in ('owner.lock', 'state.sqlite3', 'state.sqlite3-wal', 'service.sock'):
            leaf = self.desktop / name
            leaf.symlink_to(foreign)
            self.failed_serve()
            self.assertTrue(leaf.is_symlink(), name)
            self.assertEqual(foreign.read_bytes(), b'preserve these bytes')
            leaf.unlink()
        leaf = self.desktop / 'owner.lock'
        if leaf.exists():
            leaf.unlink()
        os.mkfifo(leaf, 0o600)
        self.failed_serve()
        leaf.unlink()
        os.link(foreign, leaf)
        self.failed_serve()
        leaf.unlink()
        self.desktop.chmod(0o755)
        self.failed_serve()
        self.assertEqual(self.desktop.stat().st_mode & 0o777, 0o755)

    @unittest.skipIf(os.name == 'nt', 'Unix socket transport')
    def test_truncated_oversize_and_mismatched_ipc_clients_are_refused(self):
        service, status = self.serve()
        for payload in (b'{' + b'x' * 65536, b'{}', b'{"abi":"desktop-v0"}\n',
                        json.dumps(dict(abi='desktop-v1', configuration='wrong', op='status')).encode() + b'\n'):
            with socket.socket(socket.AF_UNIX) as client:
                client.settimeout(4)
                client.connect(str(self.desktop / 'service.sock'))
                try:
                    client.sendall(payload)
                    client.shutdown(socket.SHUT_WR)
                    response = client.recv(4096)
                except ConnectionResetError:
                    response = b''
                if response:
                    self.assertEqual(json.loads(response)['t'], 'error')
        self.assertIsNone(service.poll())
        self.assertEqual(self.cli('desktop', 'status', '--json')['epoch'], status['epoch'])
        result = subprocess.run([str(HGS), 'desktop', 'request', '--json'], env=self.env,
                                input='x' * 65537, text=True, capture_output=True, timeout=5)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('exceeds its limit', result.stderr)
