"""Passive desktop-v1 kernel through real CLI processes in private state.

No system manager, native agent, production state or tmux server is used.
"""
from contextlib import closing
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
        process = subprocess.Popen([str(HGS), 'desktop', 'serve'], env=self.env,
                                   stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
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
