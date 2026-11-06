"""Candidate observations may schedule, but never manufacture a test pass."""
import importlib.util
import contextlib
import io
import json
import os
import tempfile
from unittest.mock import patch
from pathlib import Path
import unittest

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location('overlap', ROOT / 'scripts/ci/regression-overlap.py')
overlap = importlib.util.module_from_spec(spec)
spec.loader.exec_module(overlap)
SHA = 'a' * 40


class OverlapTests(unittest.TestCase):
    def observe(self, jobs, **changes):
        run = dict(id=123, head_sha=SHA, head_branch='main', head_repository=dict(full_name='ufna/zerus'),
                   event='workflow_dispatch', path='.github/workflows/release.yml', status='in_progress')
        run.update(changes)
        def fetch(path):
            if path.startswith('actions/workflows/release.yml'):
                return dict(total_count=1, workflow_runs=[run])
            if path.startswith('actions/workflows/nightly.yml'):
                return dict(total_count=0, workflow_runs=[])
            self.assertEqual(path, 'actions/runs/123/jobs?per_page=100')
            return dict(total_count=len(jobs), jobs=jobs)
        return overlap.candidate_cover(fetch, SHA)

    def test_active_or_passed_candidate_verify_suppresses_only_duplicate_scheduling(self):
        self.assertEqual(self.observe([dict(name='verify / Arch package', status='in_progress')]), 123)
        self.assertEqual(self.observe([dict(name='verify / Suite results', status='completed', conclusion='success')]), 123)
        for result in ('failure', 'cancelled', 'skipped'):
            self.assertIsNone(self.observe([dict(name='verify / Suite results', status='completed', conclusion=result)]))
        self.assertIsNone(self.observe([dict(name='macos / sign', status='in_progress')]))
        self.assertIsNone(self.observe([]))

    def test_other_source_fork_branch_event_or_path_never_suppresses_regression(self):
        for change in [dict(head_sha='b' * 40), dict(head_branch='feature'), dict(head_repository=dict(full_name='example/zerus')),
                       dict(event='pull_request'), dict(path='.github/workflows/ci.yml'), dict(status='completed')]:
            with self.subTest(change=change):
                self.assertIsNone(self.observe([dict(name='verify / Arch package', status='in_progress')], **change))

    def test_incomplete_census_is_not_claimed_as_coverage(self):
        with self.assertRaises(ValueError):
            overlap.candidate_cover(lambda _: dict(total_count=21, workflow_runs=[]), SHA)
        with self.assertRaises(ValueError):
            overlap.candidate_cover(lambda _: {}, 'not-a-commit')

    def test_unavailable_api_runs_regression_instead_of_claiming_coverage(self):
        with tempfile.TemporaryDirectory(prefix='zerus-overlap-output-') as tmp:
            output = Path(tmp) / 'output'
            with patch.dict(os.environ, SOURCE_COMMIT=SHA, GH_TOKEN='fixture', GITHUB_OUTPUT=str(output)), \
                    patch.object(overlap.urllib.request, 'urlopen', side_effect=OSError('unavailable')), \
                    contextlib.redirect_stdout(io.StringIO()) as log:
                overlap.main()
            self.assertIn('run_checks=true', output.read_text())
            self.assertEqual(json.loads(log.getvalue())['reason'], 'candidate-observation-unavailable')


if __name__ == '__main__':
    unittest.main()
