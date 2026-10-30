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
import sys
import tempfile
import threading
import time
import unittest
import uuid

HGS = Path(os.environ.get('HGS_TEST_BIN', Path(__file__).resolve().parents[1] / 'target/debug/hgs')).resolve()


class Desktop(unittest.TestCase):
    def setUp(self):
        # macOS's per-user TMPDIR (/var/folders/...) leaves too little of the
        # 104-byte socket path for the desktop service under nested state dirs.
        temp = tempfile.TemporaryDirectory(prefix='hgs-desk-', dir='/tmp' if sys.platform == 'darwin' else None)
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

    def test_schema_one_migration_preserves_configuration_and_generation(self):
        configuration = self.cli('desktop', 'status', '--json')['configuration']
        self.desktop.mkdir(mode=0o700, parents=True)
        self.state.chmod(0o700)
        database = self.desktop / 'state.sqlite3'
        with closing(sqlite3.connect(database)) as db, db:
            db.execute('CREATE TABLE service (id INTEGER PRIMARY KEY CHECK(id=1), configuration TEXT NOT NULL, generation INTEGER NOT NULL, epoch TEXT NOT NULL)')
            db.execute('INSERT INTO service VALUES(1,?,7,?)', (configuration, str(uuid.uuid4())))
            db.execute('PRAGMA user_version=1')
        database.chmod(0o600)
        process, status = self.serve()
        self.assertEqual(status['schema'], 8)
        self.assertEqual(status['generation'], 8)
        self.assertEqual(status['configuration'], configuration)
        self.assertTrue(status['effect_journal']['revoked'])
        self.assertEqual(status['effect_journal']['attempts']['reserved'], 0)
        self.assertEqual(status['read_queue']['pending'], 0)
        self.assertTrue(status['read_queue']['admissions_paused'])
        self.assertFalse(status['read_queue']['recovery_error'])
        self.stop(process)
        with closing(sqlite3.connect(database)) as db:
            self.assertEqual(db.execute('PRAGMA user_version').fetchone()[0], 8)
            self.assertEqual(db.execute('SELECT rows,bytes FROM desktop_catalog_totals').fetchone(), (0, 0))
            self.assertEqual(db.execute('SELECT count(*) FROM desktop_catalog_sources').fetchone()[0], 0)
            self.assertEqual(db.execute('SELECT configuration FROM service').fetchone()[0], configuration)

    def test_schema_five_metadata_is_charged_once_and_preserved(self):
        process, _ = self.serve()
        self.stop(process)
        database = self.desktop / 'state.sqlite3'
        hello = json.dumps({'machine_id': 'example-machine'})
        meta = json.dumps({'label': 'example-\u03bb'}, ensure_ascii=False)
        expected = len(hello.encode()) + len(meta.encode())
        with closing(sqlite3.connect(database)) as db, db:
            for suffix in ('insert', 'update', 'delete'):
                db.execute(f'DROP TRIGGER desktop_catalog_source_{suffix}')
            db.execute('INSERT INTO desktop_catalog_sources(computer,epoch,incarnation,generation,stream,seq,hello,meta) VALUES(?,?,?,?,?,?,?,?)',
                       ('peer', str(uuid.uuid4()), 1, 1, 'old-watch', 1, hello, meta))
            db.execute('DROP INDEX desktop_commands_pending')
            for table in ('desktop_operation_custody', 'desktop_operation_attempts', 'desktop_operations', 'desktop_operation_totals'):
                db.execute(f'DROP TABLE {table}')
            db.execute('PRAGMA user_version=5')
        for _ in range(2):
            process, status = self.serve()
            self.assertEqual(status['schema'], 8)
            self.assertEqual(status['catalog_projection']['stored_bytes'], expected)
            self.assertEqual(status['catalog_projection']['stored_rows'], 0)
            self.stop(process)
        with closing(sqlite3.connect(database)) as db:
            self.assertEqual(db.execute('SELECT hello,meta FROM desktop_catalog_sources').fetchone(), (hello, meta))
            self.assertEqual(db.execute('SELECT rows,bytes FROM desktop_catalog_totals').fetchone(), (0, expected))
            self.assertEqual(db.execute('SELECT count(*) FROM desktop_catalog_sources').fetchone()[0], 1)

    def test_schema_six_operation_migration_is_atomic_and_passive(self):
        process, _ = self.serve()
        self.stop(process)
        database = self.desktop / 'state.sqlite3'
        with closing(sqlite3.connect(database)) as db, db:
            db.execute('DROP INDEX desktop_commands_pending')
            for table in ('desktop_operation_custody', 'desktop_operation_attempts', 'desktop_operations', 'desktop_operation_totals'):
                db.execute(f'DROP TABLE {table}')
            db.execute('PRAGMA user_version=6')
            db.execute('CREATE TABLE desktop_operation_totals (fixture TEXT)')
        self.failed_serve()
        with closing(sqlite3.connect(database)) as db, db:
            self.assertEqual(db.execute('PRAGMA user_version').fetchone()[0], 6)
            self.assertEqual(db.execute("SELECT count(*) FROM sqlite_schema WHERE name='desktop_operations'").fetchone()[0], 0)
            db.execute('DROP TABLE desktop_operation_totals')
        process, status = self.serve()
        self.assertEqual(status['schema'], 8)
        self.assertEqual({key:status['operation_queue'][key] for key in ('unsettled','undrained','admissions_paused','recovery_error')}, dict(unsettled=0, undrained=0, admissions_paused=True, recovery_error=False))
        self.assertFalse(status['writers_enabled'])
        self.stop(process)

    def test_schema_seven_custody_migration_is_atomic_and_creates_no_lock_files(self):
        process, _ = self.serve()
        self.stop(process)
        database = self.desktop / 'state.sqlite3'
        with closing(sqlite3.connect(database)) as db, db:
            db.execute('DROP TABLE desktop_operation_custody')
            db.execute('DROP INDEX desktop_commands_pending')
            db.execute('PRAGMA user_version=7')
            db.execute('CREATE INDEX desktop_commands_pending ON desktop_commands(request_id)')
        self.failed_serve()
        with closing(sqlite3.connect(database)) as db, db:
            self.assertEqual(db.execute('PRAGMA user_version').fetchone()[0], 7)
            self.assertEqual(db.execute("SELECT count(*) FROM sqlite_schema WHERE name='desktop_operation_custody'").fetchone()[0], 0)
            self.assertEqual(db.execute("SELECT count(*) FROM sqlite_schema WHERE name LIKE 'desktop_custody_%'").fetchone()[0], 0)
            db.execute('DROP INDEX desktop_commands_pending')
        process, status = self.serve()
        self.assertEqual(status['schema'], 8)
        self.assertEqual(status['operation_queue']['undrained'], 0)
        self.assertTrue(status['operation_queue']['admissions_paused'])
        self.assertFalse((self.desktop / 'operation-custody').exists())
        self.stop(process)

    def test_catalog_collection_progresses_without_a_frontend(self):
        process, _ = self.serve()
        self.stop(process)
        with closing(sqlite3.connect(self.desktop / 'state.sqlite3')) as db, db:
            db.execute("INSERT INTO desktop_catalog_generations(computer,obsolete) VALUES('peer',1)")
            generation = db.execute('SELECT last_insert_rowid()').fetchone()[0]
            db.executemany('INSERT INTO desktop_catalog_rows(generation,key,json,bytes) VALUES(?,?,?,?)',
                           ((generation, f'run:{n}', '{}', 64) for n in range(768)))
        _, status = self.serve()
        deadline = time.monotonic() + 5
        while status['catalog_projection']['stored_rows'] and time.monotonic() < deadline:
            time.sleep(0.02)
            status = self.cli('desktop', 'status', '--json')
            self.assertEqual(status['subscribers'], 0)
            self.assertFalse(status['writers_enabled'])
        self.assertEqual(status['catalog_projection']['stored_rows'], 0)
        self.assertEqual(status['catalog_projection']['stored_bytes'], 0)
        self.assertEqual(status['catalog_projection']['obsolete_generations'], 0)
        self.assertFalse(status['catalog_projection']['collection_error'])
        self.assertTrue(status['effect_journal']['revoked'])
        self.assertEqual(status['read_queue']['pending'], 0)

    def test_revoke_without_handoff_or_service_writes_nothing(self):
        help_result = subprocess.run([str(HGS), '--help'], env=self.env, capture_output=True, text=True, timeout=5)
        self.assertEqual(help_result.returncode, 0, help_result.stderr)
        self.assertIn('Desktop revoke ABI: desktop-revoke-v1', help_result.stdout)
        self.assertFalse(self.state.exists())
        request_id = str(uuid.uuid4())
        result = self.cli('desktop', 'revoke', '--json', '--request-id', request_id, '--reason', 'rollback')
        self.assertEqual(result['handoff'], 'none')
        self.assertFalse(result['applicable'])
        self.assertTrue(result['nothing_written'])
        self.assertTrue(result['revoked'])
        self.assertEqual(result['request_id'], request_id)
        self.assertFalse(self.state.exists())
        self.assertFalse((self.root / 'unexpected-tool').exists())
        # Pre-existing native state or an unused desktop folder stays unchanged.
        self.state.mkdir(mode=0o700)
        preserved = self.state / 'foreign-state'
        preserved.write_text('preserved')
        for desktop in (False, True):
            if desktop:
                self.desktop.mkdir(mode=0o700)
            self.assertEqual(self.cli('desktop', 'revoke', '--json', '--request-id', request_id, '--reason', 'rollback'), result)
            self.assertEqual(preserved.read_text(), 'preserved')
            if desktop:
                self.assertEqual(list(self.desktop.iterdir()), [])
        orphan = self.desktop / 'state.sqlite3-wal'
        orphan.write_bytes(b'preserved orphan')
        self.cli('desktop', 'revoke', '--json', ok=False)
        self.assertEqual(orphan.read_bytes(), b'preserved orphan')
        self.assertFalse((self.desktop / 'owner.lock').exists())

    def test_absent_service_with_existing_state_revokes_without_starting_anything(self):
        process, _ = self.serve()
        self.stop(process)
        request_id = str(uuid.uuid4())
        result = self.cli('desktop', 'revoke', '--json', '--request-id', request_id, '--reason', 'rollback')
        self.assertTrue(result['revoked'])
        self.assertTrue(result['checkpointed'])
        self.assertTrue(result['durable'])
        self.assertEqual(result['request_id'], request_id)
        self.assertIsNone(result['writer_epoch'])
        self.assertFalse((self.desktop / 'handoff-v1.json').exists())
        self.assertFalse((self.desktop / 'service.sock').exists())
        self.assertFalse(self.cli('desktop', 'status', '--json')['running'])
        self.assertEqual(self.cli('desktop', 'revoke', '--json', '--request-id', request_id, '--reason', 'rollback'), result)
        self.assertFalse((self.root / 'unexpected-tool').exists())

    def test_live_revoke_preserves_import_identity_and_has_an_exact_receipt(self):
        path, record, marker = self.import_evidence()
        before_record = record.read_bytes()
        service, status = self.serve()
        request_id = str(uuid.uuid4())
        result = self.cli('desktop', 'revoke', '--json', '--request-id', request_id, '--reason', 'rollback')
        self.assertTrue(result['revoked'])
        self.assertTrue(result['checkpointed'])
        self.assertEqual(result['writer_epoch'], marker['writer_epoch'])
        marker['phase'] = 'revoked'
        self.assertEqual(json.loads(path.read_text()), marker)
        self.assertEqual(record.read_bytes(), before_record)
        query = dict(op='revoke_status', request_id=request_id)
        self.assertEqual(self.cli('desktop', 'request', '--json', data=query), result)
        current = self.cli('desktop', 'status', '--json')
        self.assertEqual(current['epoch'], status['epoch'])
        self.assertEqual(current['migration']['reason'], 'revoked')
        self.assertEqual(current['effect_journal']['reason'], 'manual_rollback')
        self.assertFalse(current['writers_enabled'])
        self.stop(service)
        self.serve()
        self.assertEqual(self.cli('desktop', 'request', '--json', data=query), result)
        self.assertFalse((self.root / 'unexpected-tool').exists())

    def test_revoke_replay_conflict_and_later_epoch_refuse_success(self):
        path, _, marker = self.import_evidence()
        self.serve()
        request_id = str(uuid.uuid4())
        args = ['desktop', 'revoke', '--json', '--request-id', request_id]
        self.assertTrue(self.cli(*args)['revoked'])
        refused = self.cli(*args, '--reason', 'rollback', ok=False)
        self.assertEqual(json.loads(refused.stdout)['code'], 'request_conflict')
        marker['writer_epoch'] = str(uuid.uuid4())
        path.write_text(json.dumps(marker))
        before = path.read_bytes()
        refused = self.cli(*args, ok=False)
        self.assertEqual(json.loads(refused.stdout)['code'], 'writer_epoch_changed')
        self.assertEqual(path.read_bytes(), before)
        self.assertTrue(self.cli('desktop', 'revoke', '--json')['revoked'])

    @unittest.skipIf(os.name == 'nt', 'Unix raw-client disconnect fixture')
    def test_revoke_disconnect_reconciles_only_the_original_request(self):
        self.import_evidence()
        _, status = self.serve()
        request_id = str(uuid.uuid4())
        command = dict(op='revoke', request_id=request_id, reason='rollback')
        hello = dict(abi='desktop-v1', configuration=status['configuration'], op='request', request=command)
        database = self.desktop / 'state.sqlite3'
        with closing(sqlite3.connect(database)) as reader, closing(sqlite3.connect(database)) as observer:
            reader.execute('BEGIN')
            reader.execute('SELECT generation FROM service').fetchone()
            with socket.socket(socket.AF_UNIX) as client:
                client.connect(str(self.desktop / 'service.sock'))
                client.sendall(json.dumps(hello).encode() + b'\n')
                # Pin actual durable admission, keeping checkpoint blocked so
                # closing the socket loses the result after acceptance. Closing
                # before hello was read need not admit anything on macOS.
                deadline = time.monotonic() + 5
                while time.monotonic() < deadline:
                    row = observer.execute('SELECT state FROM desktop_commands WHERE request_id=?', (request_id,)).fetchone()
                    if row == ('prepared',):
                        break
                    time.sleep(0.01)
                else:
                    self.fail('revoke was not durably admitted')
            reader.execute('ROLLBACK')
        # Losing the response is not permission to invent a new request. The
        # actor finishes independently, and an exact read finds that receipt.
        deadline = time.monotonic() + 5
        while time.monotonic() < deadline:
            result = self.cli('desktop', 'request', '--json', data=dict(op='revoke_status', request_id=request_id))
            if result.get('checkpointed'):
                break
            time.sleep(0.02)
        else:
            self.fail('disconnected revoke did not settle')
        self.assertEqual(self.cli('desktop', 'revoke', '--json', '--request-id', request_id, '--reason', 'rollback'), result)
        with closing(sqlite3.connect(self.desktop / 'state.sqlite3')) as db:
            self.assertEqual(db.execute('SELECT count(*) FROM desktop_commands').fetchone()[0], 1)

    def test_revoke_refuses_newer_marker_and_newer_database_without_repair(self):
        path, _, marker = self.import_evidence()
        marker['version'] = 2
        path.write_text(json.dumps(marker))
        before = path.read_bytes()
        refused = self.cli('desktop', 'revoke', '--json', ok=False)
        self.assertEqual(json.loads(refused.stdout)['code'], 'unsupported_handoff')
        self.assertEqual(path.read_bytes(), before)
        database = self.desktop / 'state.sqlite3'
        with closing(sqlite3.connect(database)) as db, db:
            db.execute('PRAGMA user_version=999')
        before = database.read_bytes()
        self.cli('desktop', 'revoke', '--json', ok=False)
        self.assertEqual(database.read_bytes(), before)

    @unittest.skipIf(os.name == 'nt', 'POSIX owner-lock contention fixture')
    def test_offline_revoke_cannot_bypass_a_live_or_unreachable_owner(self):
        import fcntl
        self.desktop.mkdir(parents=True, mode=0o700)
        self.state.chmod(0o700)
        lock = self.desktop / 'owner.lock'
        with lock.open('w') as owner:
            lock.chmod(0o600)
            fcntl.flock(owner, fcntl.LOCK_EX | fcntl.LOCK_NB)
            refused = self.cli('desktop', 'revoke', '--json', ok=False)
            self.assertIn('another desktop service', refused.stderr)
            self.assertFalse((self.desktop / 'state.sqlite3').exists())
        self.assertTrue(self.cli('desktop', 'revoke', '--json')['revoked'])

    @unittest.skipIf(os.name == 'nt', 'POSIX handoff-lock contention fixture')
    def test_live_revoke_refuses_a_busy_handoff_without_acceptance(self):
        import fcntl
        self.import_evidence()
        self.serve()
        path = self.desktop / 'handoff.lock'
        with path.open('r+') as lock:
            fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
            refused = self.cli('desktop', 'revoke', '--json', ok=False)
            response = json.loads(refused.stdout)
            self.assertFalse(response['accepted'])
            self.assertFalse(response['revoked'])
            self.assertEqual(response['code'], 'handoff_unavailable')
        self.assertTrue(self.cli('desktop', 'revoke', '--json')['revoked'])

    def test_revoke_strict_validation_and_status_never_admit_an_effect(self):
        self.serve()
        for args in [[], ['--json', '--json'], ['--json', '--request-id', 'bad'],
                     ['--json', '--reason', 'automatic'], ['--json', '--enabled']]:
            self.cli('desktop', 'revoke', *args, ok=False)
        for op in ('revoke', 'revoke_status'):
            value = dict(op=op, request_id=str(uuid.uuid4()), enabled=True)
            result = self.cli('desktop', 'request', '--json', data=value)
            self.assertFalse(result['accepted'])
            self.assertEqual(result['code'], 'invalid_revoke_request')
        result = self.cli('desktop', 'request', '--json', data=dict(op='revoke_status', request_id=str(uuid.uuid4())))
        self.assertEqual(result['code'], 'request_not_found')
        self.assertFalse(self.cli('desktop', 'status', '--json')['writers_enabled'])

    def test_service_restart_fences_prior_effects_and_keeps_manual_revocation(self):
        process, _ = self.serve()
        self.stop(process)
        database = self.desktop / 'state.sqlite3'
        with closing(sqlite3.connect(database)) as db, db:
            db.execute("INSERT INTO desktop_intents VALUES(?,'{}',1,'{}',0,0)", ('a' * 64,))
            db.execute("INSERT INTO desktop_attempts(request_id,fingerprint,intent_id,revision,epoch,service_epoch,adapter,lease,state) VALUES(?,?,?,1,?,?,?,?,'reserved')",
                       (str(uuid.uuid4()), 'b' * 64, 'a' * 64, *[str(uuid.uuid4()) for _ in range(4)]))
            db.execute("UPDATE desktop_writer SET reason='manual_rollback',revoked=1")
        _, status = self.serve()
        self.assertTrue(status['effect_journal']['revoked'])
        self.assertEqual(status['effect_journal']['reason'], 'manual_rollback')
        self.assertEqual(status['effect_journal']['attempts']['unknown'], 1)
        self.assertEqual(status['effect_journal']['attempts']['reserved'], 0)
        self.assertFalse(status['writers_enabled'])
        self.assertFalse((self.root / 'unexpected-tool').exists())

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
