"""Private staged import child; no source-store reader, native agent or manager."""
from contextlib import closing
import hashlib
import json
import os
import sqlite3
import subprocess
import time
import unittest
import uuid

import test_desktop as fixtures


def encoded(value):
    return json.dumps(value, separators=(',', ':')).encode()


def digest(value):
    return dict(sha256=hashlib.sha256(value).hexdigest(), bytes=len(value))


class ImportEvidence(unittest.TestCase):
    setUp = fixtures.Desktop.setUp
    cli = fixtures.Desktop.cli
    stop = fixtures.Desktop.stop
    serve = fixtures.Desktop.serve

    def wire(self, evidence='staged', max_bytes=1 << 20, replay=False):
        configuration = self.cli('desktop', 'status', '--json')['configuration']
        import_id = str(uuid.uuid4())
        self.import_id = import_id
        self.target = self.desktop / 'imports' / import_id
        self.snapshot = encoded(dict(private='synthetic original source body'))
        reference = digest(self.snapshot)
        page = encoded(dict(version=1, family='settings', schema=1, source_sha256='a' * 64,
                            index=0, snapshot=reference))
        self.record = encoded(dict(version=1, configuration=configuration, source='com.hgdev.hgs-tray',
                                   import_id=import_id, evidence=evidence, created_at=1, automatic_approvals=False,
                                   sources=[dict(family='settings', schema=1, format='json', sha256='a' * 64, build=None)],
                                   pages=[digest(page)]))
        header = encoded(dict(abi='desktop-import-v1', configuration=configuration, source='com.hgdev.hgs-tray',
                              replay=replay, import_id=import_id, max_bytes=max_bytes)) + b'\n'
        output = bytearray(header)
        for kind, body in [('snapshot', self.snapshot), ('page', page), ('record', self.record)]:
            fields = dict(bytes=len(body)) if kind == 'record' else dict(reference=digest(body))
            output.extend(encoded(dict(t=kind, **fields)) + b'\n' + body + b'\n')
        return bytes(output)

    def publish(self, data, ok=True):
        result = subprocess.run([str(fixtures.HGS), 'desktop', 'import-evidence', '--json'], env=self.env,
                                input=data, capture_output=True, timeout=15)
        if ok:
            self.assertEqual(result.returncode, 0, result.stderr.decode())
            return json.loads(result.stdout)
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(result.stdout, b'')
        self.assertNotIn(b'synthetic original source body', result.stderr)
        return result

    def test_help_and_import_status_are_read_only_before_any_publication(self):
        result = subprocess.run([str(fixtures.HGS), '--help'], env=self.env, capture_output=True, text=True, timeout=5)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn('Desktop import ABI: desktop-import-v1', result.stdout)
        result = self.cli('desktop', 'import-status', '--json', '--import-id', str(uuid.uuid4()))
        self.assertFalse(result['record_present'])
        self.assertFalse(self.state.exists())
        self.cli('desktop', 'import-status', '--json', '--import-id', '../escape', ok=False)
        self.assertFalse(self.state.exists())

    def test_private_immutable_staged_bundle_never_writes_marker_or_database(self):
        data = self.wire()
        source = self.root / 'source.json'
        source.write_bytes(self.snapshot)
        result = self.publish(data)
        self.assertTrue(result['published'])
        self.assertEqual(result['evidence'], 'staged')
        self.assertEqual(result['import_id'], self.import_id)
        self.assertEqual((self.target / 'record-v1.json').read_bytes(), self.record)
        self.assertEqual(source.read_bytes(), self.snapshot)
        for name in ('owner.lock', 'state.sqlite3', 'handoff-v1.json', 'service.sock'):
            self.assertFalse((self.desktop / name).exists(), name)
        self.assertFalse(self.cli('desktop', 'status', '--json')['running'])
        observed = self.cli('desktop', 'import-status', '--json', '--import-id', self.import_id)
        self.assertTrue(observed['record_present'])
        self.assertEqual(observed['import_record'], result['import_record'])
        self.assertNotIn('synthetic original source body', json.dumps(observed))
        self.publish(data, ok=False)
        self.assertEqual((self.target / 'record-v1.json').read_bytes(), self.record)
        if os.name != 'nt':
            for path in self.target.rglob('*'):
                self.assertEqual(path.stat().st_mode & 0o777, 0o700 if path.is_dir() else 0o600)
        self.assertFalse((self.root / 'unexpected-tool').exists())

    def test_truncated_trailing_drained_replay_and_budget_refuse_without_commit(self):
        for change in ('truncated', 'trailing', 'drained', 'replay', 'budget'):
            with self.subTest(change=change):
                data = self.wire(evidence='drained' if change == 'drained' else 'staged',
                                 max_bytes=1 if change == 'budget' else 1 << 20, replay=change == 'replay')
                if change == 'truncated':
                    data = data[:-3]
                if change == 'trailing':
                    data += b'future'
                self.publish(data, ok=False)
                self.assertFalse(self.target.exists())
                self.assertFalse((self.desktop / 'handoff-v1.json').exists())

    def test_publishing_while_passive_service_runs_preserves_its_single_owner(self):
        service, status = self.serve()
        result = self.publish(self.wire())
        self.assertEqual(result['evidence'], 'staged')
        current = self.cli('desktop', 'status', '--json')
        self.assertEqual(current['epoch'], status['epoch'])
        self.assertEqual(current['upstreams'], [])
        self.assertFalse(current['writers_enabled'])
        with closing(sqlite3.connect(self.desktop / 'state.sqlite3')) as db:
            self.assertEqual(db.execute('SELECT count(*) FROM desktop_commands').fetchone()[0], 0)
        self.stop(service)
        self.assertFalse((self.root / 'unexpected-tool').exists())

    def test_stalled_child_exits_on_idle_deadline_without_publishing(self):
        data = self.wire()
        child = subprocess.Popen([str(fixtures.HGS), 'desktop', 'import-evidence', '--json'], env=self.env,
                                 stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        def close():
            if child.poll() is None:
                child.kill()  # Only this isolated publisher child belongs to this test.
            child.communicate(timeout=5)
        self.addCleanup(close)
        child.stdin.write(data.split(b'\n', 1)[0] + b'\n')
        child.stdin.flush()
        start = time.monotonic()
        child.wait(timeout=14)
        self.assertNotEqual(child.returncode, 0)
        self.assertLess(time.monotonic() - start, 14)
        self.assertFalse(self.target.exists())
        self.assertEqual(child.stdout.read(), b'')
        self.assertFalse((self.desktop / 'owner.lock').exists())
