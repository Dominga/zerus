"""Bounded original-result route through a real private passive CLI service.

No frontend admission, task delivery, native agent or platform manager is used.
"""
from contextlib import closing
import json
from pathlib import Path
import sqlite3
import sys
import unittest
import uuid

import test_desktop as kernel


@unittest.skipUnless(sys.platform.startswith(('linux', 'darwin')), 'Unix TaskStart actor')
class TaskStartResult(unittest.TestCase):
    setUp = kernel.Desktop.setUp
    cli = kernel.Desktop.cli
    serve = kernel.Desktop.serve
    stop = kernel.Desktop.stop

    def test_missing_original_is_an_exact_read_not_admission_or_a_new_offer(self):
        service, initial = self.serve()
        fixture = json.loads((Path(__file__).resolve().parents[1] /
                              'client/fixtures/desktop-operation-port-v1.json').read_text())
        binding = fixture['binding']
        binding['key']['configuration'] = initial['configuration']
        scope = dict(configuration=initial['configuration'], service_epoch=initial['epoch'])
        query = dict(request_id=str(uuid.uuid4()), scope=scope,
                     reconcile_id=str(uuid.uuid4()), binding=binding)
        body = dict(op='task_start_reconcile_result', query=query)
        answer = self.cli('desktop', 'request', '--json', data=body)
        self.assertEqual(answer, dict(abi='desktop-task-start-result-v1',
                         request_id=query['request_id'], scope=scope, read_only=True,
                         found=False, attempt=None))
        self.assertLess(len(json.dumps(answer).encode()), 16 << 10)
        changed = json.loads(json.dumps(body))
        changed['query']['scope']['service_epoch'] = str(uuid.uuid4())
        refused = self.cli('desktop', 'request', '--json', data=changed)
        self.assertEqual(refused, dict(accepted=False, read_only=True,
                         code='task_start_result_unavailable'))
        changed = dict(body, permit=True)
        self.assertEqual(self.cli('desktop', 'request', '--json', data=changed), refused)
        with closing(sqlite3.connect(self.desktop / 'state.sqlite3')) as db:
            for table in ('desktop_operations', 'desktop_operation_attempts',
                          'desktop_operation_custody', 'desktop_commands'):
                self.assertEqual(db.execute('SELECT count(*) FROM '+table).fetchone()[0], 0)
        current = self.cli('desktop', 'status', '--json')
        self.assertEqual(current['epoch'], initial['epoch'])
        self.assertEqual(current['desktop_ownership']['revision'], 0)
        self.assertFalse(current['writers_enabled'])
        self.assertIsNone(service.poll())
        self.assertFalse((self.root / 'unexpected-tool').exists())


if __name__ == '__main__':
    unittest.main()
