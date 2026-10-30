"""Cached GUI read health, never a producer permit; private canonical sockets."""
import json
import os
import socket
import threading
import time
import unittest
import test_desktop as kernel


@unittest.skipIf(os.name == 'nt', 'Unix canonical endpoint; Windows remains unknown')
class Frontend(unittest.TestCase):
    cli = kernel.Desktop.cli
    serve = kernel.Desktop.serve
    stop = kernel.Desktop.stop

    def setUp(self):
        kernel.Desktop.setUp(self)
        self.path = self.root / 'tmp/hgs-tray'
        self.configuration = self.cli('desktop', 'status', '--json')['configuration']

    def peer(self, transform=lambda v: v):
        server = socket.socket(socket.AF_UNIX)
        server.bind(str(self.path))
        server.listen(4)
        server.settimeout(.1)
        stop = threading.Event()
        self.requests = []
        self.errors = []

        def worker():
            while not stop.is_set():
                try:
                    stream, _ = server.accept()
                except socket.timeout:
                    continue
                try:
                    with stream:
                        stream.settimeout(2)
                        line = b''
                        while not line.endswith(b'\n') and len(line) <= 64:
                            part = stream.recv(1)
                            if not part:
                                break
                            line += part
                        self.requests.append(line)
                        self.assertTrue(line.startswith(b'desktop-producer-v1 '))
                        self.assertLessEqual(len(line), 64)
                        nonce = line.decode().strip().split(' ')[1]
                        value = dict(abi='desktop-producer-v1', version=1, challenge=None,
                            hello=dict(abi='desktop-delegation-v1', version=1, nonce=nonce,
                                application='com.hgdev.hgs-tray', gui='tauri',
                                build=dict(version='1.2.3', source_commit='b' * 40, source_dirty=False),
                                instance_epoch='22222222-2222-4222-8222-222222222222', writer_epoch=None,
                                configuration=self.configuration, live=True, replay=False),
                            read_delivery=dict(revision=5, capacity=4, in_flight=4, pinned=4, stopped=False))
                        body = transform(value)
                        if body is not None:
                            stream.sendall((json.dumps(body) + '\n').encode())
                except Exception as error:
                    self.errors.append(str(error))
        thread = threading.Thread(target=worker)
        thread.start()

        def close():
            stop.set()
            thread.join(timeout=3)
            server.close()
            self.assertFalse(thread.is_alive())
            self.assertEqual(self.errors, [])
        self.addCleanup(close)

    def health(self):
        status = self.cli('desktop', 'status', '--json')
        self.assertFalse(status['writers_enabled'])
        result = status['frontend_read_delivery']
        self.assertFalse(result['authority'])
        return result

    def test_static_status_reads_bounded_health_without_creating_desktop_state(self):
        self.peer()
        health = self.health()
        self.assertEqual(health['state'], 'live_next')
        self.assertEqual(health['read_delivery']['pinned'], 4)
        self.assertEqual(health['read_delivery']['capacity'], 4)
        self.health()
        self.assertNotEqual(self.requests[0], self.requests[1], 'fresh per-probe nonce')
        self.assertFalse(self.state.exists())

    def test_service_caches_health_and_does_not_probe_on_each_status_read(self):
        self.peer()
        self.serve()
        until = time.monotonic() + 3
        while self.health()['state'] != 'live_next' and time.monotonic() < until:
            time.sleep(.02)
        self.assertEqual(self.health()['read_delivery']['pinned'], 4)
        before = len(self.requests)
        for _ in range(5):
            self.health()
        self.assertEqual(len(self.requests), before)

    def test_live_legacy_and_malformed_health_remain_unknown(self):
        self.peer(lambda v: None)
        self.assertEqual(self.health()['state'], 'live_legacy_or_unknown')
        self.assertFalse(self.state.exists())

    def test_invalid_counts_or_wrong_configuration_cannot_publish_health(self):
        def invalid(v):
            v['read_delivery']['pinned'] = 5
            return v
        self.peer(invalid)
        self.assertEqual(self.health()['state'], 'live_legacy_or_unknown')
        self.assertFalse(self.state.exists())


if __name__ == '__main__':
    unittest.main()
