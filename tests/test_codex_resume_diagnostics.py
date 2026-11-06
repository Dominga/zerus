"""Bounds and original-launch binding of private Codex resume diagnostics."""
import json
from pathlib import Path
import sqlite3
import tempfile
import unittest

from codex_resume_diagnostics import LIMIT, METADATA_LIMIT, ResumeDiagnostics, sqlite_log_tail


class ResumeEvidence(unittest.TestCase):
    def original(self):
        return dict(run_id='original', conversation_id='conversation', base=['codex', 'fixture'])

    def test_ended_error_is_bound_to_the_original_observed_launch(self):
        diagnostic = ResumeDiagnostics('codex/fixture', self.original())
        requests = []
        def query(request):
            requests.append(request)
            if request['op'] == 'list':
                return dict(sessions=[dict(id='$1', name='codex/fixture', launch_id='original-host-launch')])
            return dict(session='$1', pane='%1', name='codex/fixture', ended=True,
                        dead=True, code=1, output='Synthetic original error')
        resumed = dict(run_id='new', expected_id='conversation', base=['codex', 'resume', 'conversation'])
        diagnostic.sample(resumed, query)
        self.assertEqual(diagnostic.evidence['original_launch_status']['code'], 1)
        self.assertEqual(diagnostic.evidence['original_launch_status']['output'], 'Synthetic original error')
        self.assertEqual(requests, [dict(op='list'), dict(op='launch_status', launch_id='original-host-launch')])

    def test_old_or_foreign_bindings_and_replaced_launches_supply_no_error(self):
        diagnostic = ResumeDiagnostics('codex/fixture', self.original())
        def forbidden(request):
            self.fail('The wrong resume must not query a launch')
        for record in [self.original(), dict(run_id='new', expected_id='elsewhere'), {}]:
            diagnostic.sample(record, forbidden)
        def replaced(request):
            if request['op'] == 'list':
                return dict(sessions=[dict(id='$1', name='codex/fixture', launch_id='original-host-launch')])
            return dict(session='$2', name='codex/fixture', ended=True, code=1, output='foreign')
        diagnostic.sample(dict(run_id='new', expected_id='conversation'), replaced)
        self.assertNotIn('original_launch_status', diagnostic.evidence)

    def test_file_evidence_is_bounded_read_only_and_omits_session_content(self):
        with tempfile.TemporaryDirectory() as folder:
            home = Path(folder);(home / 'log').mkdir()
            log = home / 'log/codex-tui.log'
            log.write_bytes(b'x' * (2 * LIMIT) + b'synthetic tail')
            source = home / 'rollout.jsonl'
            original = dict(self.original(), transcript=str(source))
            source.write_text(json.dumps(dict(type='session_meta', payload=dict(id='conversation', cwd='/fixture',
                cli_version='0.167.0', instructions='Do not include this field' * 8000))) + '\nSynthetic conversation content\n')
            before = source.read_bytes()
            diagnostic = ResumeDiagnostics('codex/fixture', original)
            result = json.loads(diagnostic.finish(home))
            self.assertLessEqual(len(result['native_log_tail'].encode()), LIMIT)
            self.assertTrue(result['native_log_tail'].endswith('synthetic tail'))
            self.assertEqual(result['original_session_file']['metadata']['id'], 'conversation')
            self.assertNotIn('Do not include', json.dumps(result))
            self.assertNotIn('conversation content', json.dumps(result))
            self.assertEqual(source.read_bytes(), before)
            source.write_text('x' * (METADATA_LIMIT + 1))
            result = json.loads(diagnostic.finish(home))
            self.assertEqual(result['original_session_file'], {'unavailable': 'ValueError'})

    def test_sqlite_logs_use_a_finite_recent_window_and_release_the_database(self):
        with tempfile.TemporaryDirectory() as folder:
            home = Path(folder)
            self.assertIn('unavailable', sqlite_log_tail(home))
            path = home / 'logs_2.sqlite'
            self.assertFalse(path.exists(), 'the read-only diagnostic must not create a database')
            database = sqlite3.connect(path)
            try:
                database.execute('CREATE TABLE logs (id INTEGER PRIMARY KEY, level TEXT, target TEXT, '
                                 'feedback_log_body TEXT, process_uuid TEXT)')
                database.execute('INSERT INTO logs VALUES (1, ?, ?, ?, ?)',
                                 ('ERROR', 'fixture', 'outside the recent window', 'old'))
                database.executemany('INSERT INTO logs VALUES (?, ?, ?, ?, ?)',
                    [(i, 'INFO', 'fixture', 'not diagnostic', 'original') for i in range(2, 66)])
                database.execute('INSERT INTO logs VALUES (66, ?, ?, ?, ?)',
                                 ('ERROR', 'fixture', 'original resume failed', 'original'))
                database.commit()
            finally:
                database.close()
            before = path.read_bytes()
            result = sqlite_log_tail(home)
            self.assertEqual(result, dict(rows=[('ERROR', 'fixture', 'original resume failed', 'original')],
                                          truncated=False, window=64))
            self.assertEqual(path.read_bytes(), before)
            # On Windows an open SQLite connection would prevent removal, even after its
            # context manager committed. The diagnostic must close the actual connection.
            path.unlink()
            self.assertFalse(path.exists())

    def test_sqlite_log_fields_and_total_bytes_are_bounded(self):
        with tempfile.TemporaryDirectory() as folder:
            home = Path(folder)
            with sqlite3.connect(home / 'logs_2.sqlite') as database:
                database.execute('CREATE TABLE logs (id INTEGER PRIMARY KEY, level TEXT, target TEXT, '
                                 'feedback_log_body TEXT, process_uuid TEXT)')
                database.executemany('INSERT INTO logs VALUES (?, ?, ?, ?, ?)',
                    [(i, 'ERROR', 'fixture', '界' * 9000, 'original') for i in range(64)])
            database.close()
            result = sqlite_log_tail(home)
            self.assertTrue(result['truncated'])
            self.assertLessEqual(len(json.dumps(result['rows'], ensure_ascii=False).encode()), LIMIT)
            self.assertTrue(all(len(row[2]) == 2048 for row in result['rows']))
