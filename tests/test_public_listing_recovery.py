"""Public catalog cleanup keeps exact canonical recovery/inspection evidence."""
import json
import shutil
import sys
import unittest

from test_listing_index import ListingIndex


KEYS = ('recovery_user_text', 'recovery_user_at', 'recovery_success_at',
        'recovery_cancel_at', 'keep_warm_prompt_at')


@unittest.skipUnless(sys.platform.startswith('linux') and shutil.which('tmux'),
                     'private qualified Linux listing fixture')
class PublicListingRecovery(unittest.TestCase):
    tearDown = ListingIndex.tearDown
    saved = ListingIndex.saved
    ready = ListingIndex.ready
    complete = ListingIndex.complete
    call = ListingIndex.call
    canonical = ListingIndex.canonical

    def setUp(self):
        ListingIndex.setUp(self)
        # Ordinary compatibility is explicit and small. Indexed controls
        # enable their own worker, rather than racing an incidental bootstrap.
        directory = self.state/'listing-metadata'
        directory.mkdir(mode=0o700)
        disabled = directory/'disabled'
        disabled.write_text('1\n')
        disabled.chmod(0o600)

    def control(self, indexed):
        record, path = self.saved(1)
        record.update(dict(zip(KEYS, ['Exact native recovery: "quoted"\nКириллица',
                                     1700000000, 1700000001, 1700000002, 1700000003])))
        path.write_text(json.dumps(record))
        original = path.read_bytes()
        if indexed:
            self.ready()
            rows = self.complete()['sessions']
        else:
            # __state merge is the existing canonical/watch builder, independent
            # of ordinary ls automatic cache activation.
            rows = self.canonical()
        self.assertEqual([row['name'] for row in rows], [record['name']])
        row = rows[0]
        self.assertFalse(set(KEYS) & row.keys(), 'raw recovery evidence leaked into public catalog')
        selected = self.call('inspect', record['name'], '--skip-processes')
        self.assertEqual(selected.returncode, 0, selected.stderr)
        selected = json.loads(selected.stdout)
        self.assertEqual({key: selected[key] for key in KEYS},
                         {key: record[key] for key in KEYS})
        for key in ('run_id', 'conversation_id', 'session_id', 'phase', 'activity',
                    'reply_id', 'recovery',
                    'recovery_capability', 'recovery_error_class'):
            self.assertEqual(row.get(key), selected.get(key), key)
        self.assertEqual(path.read_bytes(), original, 'public cleanup changed canonical evidence')
        if indexed:
            import sqlite3
            from contextlib import closing
            with closing(sqlite3.connect(self.state/'listing-metadata/index.sqlite3')) as db:
                seed = json.loads(db.execute('SELECT seed FROM sources WHERE source=?',
                                            (path.name,)).fetchone()[0])
            self.assertEqual({key: seed['record'][key] for key in KEYS},
                             {key: record[key] for key in KEYS})

    def test_canonical_public_catalog_keeps_selected_recovery_exact(self):
        self.control(False)

    def test_indexed_public_catalog_keeps_seed_and_selected_recovery_exact(self):
        self.control(True)

    def model_choices_control(self, indexed):
        home = self.root/'codex-home'
        home.mkdir()
        choices = [dict(id='fixture-a', label='Fixture A', effort_options=['low', 'high']),
                   dict(id='fixture-b', label='Fixture B', effort_options=['high'])]
        cache = home/'models_cache.json'
        cache.write_text(json.dumps(dict(models=[
            dict(slug=row['id'], display_name=row['label'],
                 supported_reasoning_levels=[dict(effort=level) for level in row['effort_options']])
            for row in choices])))
        records = []
        for n, reason in ((1, 'interrupted'), (2, 'paused')):
            record, path = self.saved(n)
            record.update(agent_home=str(home), model='fixture-a', effort='high',
                          model_options=choices, exit_reason=reason, paused=reason == 'paused')
            path.write_text(json.dumps(record))
            records.append((record, path, path.read_bytes()))
        if indexed:
            self.ready()
            import sqlite3
            from contextlib import closing
            with closing(sqlite3.connect(self.state/'listing-metadata/index.sqlite3')) as db:
                seeds = dict(db.execute('SELECT source,seed FROM sources'))
        rows = self.complete()['sessions']
        self.assertEqual({row['name'] for row in rows}, {record['name'] for record, _, _ in records})
        for record, path, original in records:
            with self.subTest(indexed=indexed, reason=record['exit_reason']):
                selected = self.call('inspect', record['name'], '--skip-processes')
                self.assertEqual(selected.returncode, 0, selected.stderr)
                selected = json.loads(selected.stdout)
                self.assertEqual(selected.get('model_options'), choices)
                self.assertEqual(selected['effort_options'], ['low', 'high'])
                row = next(row for row in rows if row['name'] == record['name'])
                self.assertNotIn('model_options', row, 'selected-only choices leaked into public catalog')
                for key in ('model', 'effort', 'effort_options', 'settings_change_supported',
                            'settings_change_reason', 'settings_apply_when', 'reply_id',
                            'recovery', 'recovery_capability', 'recovery_error_class'):
                    self.assertEqual(row.get(key), selected.get(key), key)
                self.assertEqual(path.read_bytes(), original)
                self.assertEqual(json.loads(path.read_text())['model_options'], choices)
        if indexed:
            import sqlite3
            from contextlib import closing
            with closing(sqlite3.connect(self.state/'listing-metadata/index.sqlite3')) as db:
                for _, path, _ in records:
                    seed = json.loads(db.execute('SELECT seed FROM sources WHERE source=?',
                                                (path.name,)).fetchone()[0])
                    self.assertEqual(seed, json.loads(seeds[path.name]))

    def test_compatibility_list_keeps_saved_and_paused_selected_model_choices(self):
        self.model_choices_control(False)

    def test_indexed_list_keeps_saved_and_paused_selected_model_choices(self):
        self.model_choices_control(True)


if __name__ == '__main__':
    unittest.main()
