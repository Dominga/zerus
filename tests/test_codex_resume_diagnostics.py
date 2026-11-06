"""Bounds and original-launch binding of private Codex resume diagnostics."""
import json
from pathlib import Path
import tempfile
import unittest

from codex_resume_diagnostics import LIMIT, ResumeDiagnostics


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
                cli_version='0.167.0', instructions='Do not include this field'))) + '\nSynthetic conversation content\n')
            before = source.read_bytes()
            diagnostic = ResumeDiagnostics('codex/fixture', original)
            result = json.loads(diagnostic.finish(home))
            self.assertLessEqual(len(result['native_log_tail'].encode()), LIMIT)
            self.assertTrue(result['native_log_tail'].endswith('synthetic tail'))
            self.assertEqual(result['original_session_file']['metadata']['id'], 'conversation')
            self.assertNotIn('Do not include', json.dumps(result))
            self.assertNotIn('conversation content', json.dumps(result))
            self.assertEqual(source.read_bytes(), before)
            source.write_text('x' * 9000)
            result = json.loads(diagnostic.finish(home))
            self.assertEqual(result['original_session_file'], {'unavailable': 'ValueError'})
