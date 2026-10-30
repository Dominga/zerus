"""Legacy presence admission in private state; no native sampler or service unit."""
from contextlib import closing
import json
import os
import sqlite3
import unittest

import test_desktop as fixtures


class PresenceAdmission(unittest.TestCase):
    setUp = fixtures.Desktop.setUp
    cli = fixtures.Desktop.cli
    stop = fixtures.Desktop.stop
    serve = fixtures.Desktop.serve
    import_evidence = fixtures.Desktop.import_evidence

    def presence(self, locked=False, ok=True):
        return self.cli('presence', 'set', '--json', data=dict(idle_seconds=12.5, locked=locked), ok=ok)

    def test_legacy_payload_and_output_stay_compatible_without_desktop_state(self):
        self.assertEqual(self.presence(True), dict(ok=True))
        path = self.state / 'desktop-presence.json'
        first = json.loads(path.read_text())
        self.assertTrue(first['locked'])
        self.assertEqual(first['idle_seconds'], 12.5)
        self.assertEqual(self.presence(True), dict(ok=True))
        self.assertEqual(json.loads(path.read_text())['locked_since'], first['locked_since'])
        self.assertEqual(self.presence(), dict(ok=True))
        self.assertIsNone(json.loads(path.read_text())['locked_since'])
        self.assertFalse(self.desktop.exists())
        if os.name != 'nt':
            self.assertEqual((self.state / 'desktop-presence.lock').stat().st_mode & 0o777, 0o600)
        self.assertFalse((self.root / 'unexpected-tool').exists())

    def test_supported_handoff_fences_legacy_until_explicit_revoke(self):
        self.presence()
        path = self.state / 'desktop-presence.json'
        before = path.read_bytes()
        marker_path, _, marker = self.import_evidence()
        for phase in ('imported', 'quiesced', 'complete'):
            marker['phase'] = phase
            marker_path.write_text(json.dumps(marker))
            refusal = self.presence(True, ok=False)
            self.assertIn('hgs desktop revoke --json', refusal.stderr)
            self.assertEqual(path.read_bytes(), before)
        self.assertTrue(self.cli('desktop', 'revoke', '--json')['checkpointed'])
        self.assertEqual(self.presence(True), dict(ok=True))
        self.assertTrue(json.loads(path.read_text())['locked'])

    def test_passive_service_permits_legacy_but_active_policy_refuses(self):
        process, status = self.serve()
        self.assertEqual(self.presence(), dict(ok=True))
        before = (self.state / 'desktop-presence.json').read_bytes()
        with closing(sqlite3.connect(self.desktop / 'state.sqlite3')) as db, db:
            db.execute('UPDATE desktop_writer SET revoked=0')
        refusal = self.presence(True, ok=False)
        self.assertIn('hgs desktop revoke --json', refusal.stderr)
        self.assertEqual((self.state / 'desktop-presence.json').read_bytes(), before)
        current = self.cli('desktop', 'status', '--json')
        self.assertEqual(current['epoch'], status['epoch'])
        self.assertFalse(current['writers_enabled'])
        self.assertFalse(current['effect_journal']['revoked'])
        self.assertTrue(self.cli('desktop', 'revoke', '--json')['revoked'])
        self.assertEqual(self.presence(), dict(ok=True))
        self.stop(process)

    def test_unknown_marker_and_newer_database_stay_unchanged(self):
        self.presence()
        before = (self.state / 'desktop-presence.json').read_bytes()
        path, _, marker = self.import_evidence()
        marker['version'] = 999
        path.write_text(json.dumps(marker))
        marker_bytes = path.read_bytes()
        self.presence(True, ok=False)
        self.assertEqual(path.read_bytes(), marker_bytes)
        self.assertEqual((self.state / 'desktop-presence.json').read_bytes(), before)
        path.unlink()
        process, _ = self.serve()
        self.stop(process)
        database = self.desktop / 'state.sqlite3'
        with closing(sqlite3.connect(database)) as db, db:
            db.execute('PRAGMA user_version=999')
        before_db = database.read_bytes()
        self.presence(True, ok=False)
        self.assertEqual(database.read_bytes(), before_db)
        self.assertEqual((self.state / 'desktop-presence.json').read_bytes(), before)

    @unittest.skipIf(os.name == 'nt', 'POSIX lock contention and unsafe-leaf fixtures')
    def test_contention_and_unsafe_lock_refuse_without_touching_presence(self):
        import fcntl
        self.presence()
        path = self.state / 'desktop-presence.json'
        before = path.read_bytes()
        lock_path = self.state / 'desktop-presence.lock'
        with lock_path.open('r+') as lock:
            fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
            self.presence(True, ok=False)
        self.assertEqual(path.read_bytes(), before)
        lock_path.unlink()
        foreign = self.root / 'foreign'
        foreign.write_bytes(b'preserved')
        lock_path.symlink_to(foreign)
        self.presence(True, ok=False)
        self.assertEqual(foreign.read_bytes(), b'preserved')
        self.assertEqual(path.read_bytes(), before)
        self.assertTrue(lock_path.is_symlink())
