"""Read-only canonical owner challenge in isolated state and Unix sockets."""
import json
import os
import socket
import subprocess
import threading
import unittest
import test_desktop as kernel


@unittest.skipIf(os.name == 'nt', 'Unix canonical socket; Windows stays unknown')
class Owner(unittest.TestCase):
    setUp = kernel.Desktop.setUp
    cli = kernel.Desktop.cli

    def setUp(self):
        kernel.Desktop.setUp(self)
        self.env['TMPDIR'] = str(self.root)
        self.path = self.root / 'hgs-tray'

    def probe(self):
        value = self.cli('desktop', 'owner-status', '--json')
        self.assertFalse(value['authority'])
        self.assertFalse(value['takeover'])
        self.assertFalse(value['writers_enabled'])
        self.assertFalse(self.state.exists())
        return value['observation']

    def peer(self, reply):
        server = socket.socket(socket.AF_UNIX)
        server.bind(str(self.path))
        server.listen(1)
        server.settimeout(4)
        self.addCleanup(server.close)
        received = []
        errors = []

        def run():
            try:
                with server.accept()[0] as stream:
                    stream.settimeout(3)
                    line = b''
                    while not line.endswith(b'\n') and len(line) <= 64:
                        byte = stream.recv(1)
                        if not byte:
                            break
                        line += byte
                    received.append(line)
                    body = reply(line)
                    if body:
                        stream.sendall(body)
            except Exception as error:
                errors.append(error)

        worker = threading.Thread(target=run)
        worker.start()
        result = self.probe()
        worker.join(timeout=4)
        self.assertFalse(worker.is_alive())
        self.assertEqual(errors, [])
        self.assertEqual(len(received), 1)
        self.assertEqual(len(received[0]), 59)
        self.assertTrue(self.path.exists(), 'probe never deletes the canonical socket')
        return result, received[0]

    def test_exact_capability_absence_and_unsafe_entries_write_nothing(self):
        help_result = subprocess.run([str(kernel.HGS), '--help'], env=self.env, capture_output=True, text=True, timeout=5)
        self.assertIn('Desktop owner ABI: desktop-delegation-v1', [s.strip() for s in help_result.stdout.splitlines()])
        self.assertEqual(self.probe()['state'], 'absent')
        self.path.write_text('existing')
        self.assertEqual(self.probe()['state'], 'unknown')
        self.assertEqual(self.path.read_text(), 'existing')

    def test_legacy_rejection_remains_a_live_unknown_instance(self):
        result, _ = self.peer(lambda _: b'')
        self.assertEqual(result['state'], 'live_legacy_or_unknown')

    def test_exact_next_identity_and_fresh_nonce_on_each_probe(self):
        configuration = self.cli('desktop', 'status', '--json')['configuration']

        def reply(line):
            nonce = line.decode().strip().split(' ')[1]
            return (json.dumps(dict(abi='desktop-delegation-v1', version=1, nonce=nonce,
                application='com.hgdev.hgs-tray', gui='tauri',
                build=dict(version='1.2.3', source_commit='b' * 40, source_dirty=False),
                instance_epoch='22222222-2222-4222-8222-222222222222', writer_epoch=None,
                configuration=configuration, live=True, replay=False)) + '\n').encode()

        result, first = self.peer(reply)
        self.assertEqual(result['state'], 'live_next')
        self.path.unlink()  # fixture alone owns this socket
        result, second = self.peer(reply)
        self.assertEqual(result['state'], 'live_next')
        self.assertNotEqual(first, second)

    def test_mismatched_and_oversized_replies_are_passive(self):
        result, _ = self.peer(lambda _: b'{"abi":"unknown"}\n')
        self.assertEqual(result['state'], 'live_legacy_or_unknown')
        self.path.unlink()
        result, _ = self.peer(lambda _: b'x' * 4097)
        self.assertEqual(result['state'], 'live_legacy_or_unknown')
