"""Candidate observations may schedule, but never manufacture a test pass."""
import importlib.util
import contextlib
import io
import json
import os
import re
from itertools import product
from types import SimpleNamespace
import tempfile
import http.client
import http.server
import threading
import urllib.request
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
        workflow = changes.pop('workflow', 'release.yml')
        run = dict(id=123, head_sha=SHA, head_branch='main', head_repository=dict(full_name='ufna/zerus'),
                   event='workflow_dispatch', path='.github/workflows/' + workflow, status='in_progress')
        run.update(changes)
        def fetch(path):
            if path.startswith('actions/workflows/' + workflow):
                return dict(total_count=1, workflow_runs=[run])
            if path.startswith('actions/workflows/'):
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
            self.assertIn('covered=false', output.read_text())
            self.assertEqual(json.loads(log.getvalue())['reason'], 'candidate-observation-unavailable')

    def test_manual_nightly_observes_review_while_scheduled_nightly_observes_verify(self):
        for event, prefix in [('workflow_dispatch', 'review / '), ('schedule', 'verify / ')]:
            with self.subTest(event=event):
                self.assertEqual(self.observe([dict(name=prefix + 'Arch package', status='in_progress')], workflow='nightly.yml', event=event), 123)
                self.assertEqual(self.observe([dict(name=prefix + 'Suite results', status='completed', conclusion='success')], workflow='nightly.yml', event=event), 123)
                self.assertIsNone(self.observe([dict(name=prefix + 'Suite results', status='completed', conclusion='failure')], workflow='nightly.yml', event=event))

    def test_real_truncated_chunked_response_explicitly_runs_regression(self):
        class Truncated(http.server.BaseHTTPRequestHandler):
            protocol_version = 'HTTP/1.1'
            def log_message(self, *_): pass
            def do_GET(self):
                self.send_response(200)
                self.send_header('Transfer-Encoding', 'chunked')
                self.send_header('Connection', 'close')
                self.end_headers()
                self.wfile.write(b'10\r\nabc')  # Deliberately incomplete chunk.
                self.wfile.flush()
                self.close_connection = True
        with http.server.HTTPServer(('127.0.0.1', 0), Truncated) as server, \
                tempfile.TemporaryDirectory(prefix='zerus-truncated-overlap-') as tmp:
            server.timeout = 2
            thread = threading.Thread(target=server.handle_request, daemon=True)
            thread.start()
            opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))
            output = Path(tmp) / 'output'
            def loopback(_request, timeout):
                return opener.open('http://127.0.0.1:' + str(server.server_port), timeout=timeout)
            try:
                with patch.dict(os.environ, SOURCE_COMMIT=SHA, GH_TOKEN='fixture', GITHUB_OUTPUT=str(output)), \
                        patch.object(overlap.urllib.request, 'urlopen', side_effect=loopback), \
                        contextlib.redirect_stdout(io.StringIO()):
                    try:
                        overlap.main()
                    except Exception as error:
                        self.fail('Observer did not fail open: ' + type(error).__name__)
                self.assertIn('run_checks=true', output.read_text())
                self.assertIn('covered=false', output.read_text())
            finally:
                thread.join(3)
                self.assertFalse(thread.is_alive(), 'Original loopback thread did not exit')

    def test_incomplete_read_exception_is_not_a_positive_coverage_output(self):
        with tempfile.TemporaryDirectory(prefix='zerus-incomplete-overlap-') as tmp:
            output = Path(tmp) / 'output'
            with patch.dict(os.environ, SOURCE_COMMIT=SHA, GH_TOKEN='fixture', GITHUB_OUTPUT=str(output)), \
                    patch.object(overlap, 'candidate_cover', side_effect=http.client.IncompleteRead(b'partial')), \
                    contextlib.redirect_stdout(io.StringIO()):
                try:
                    overlap.main()
                except Exception as error:
                    self.fail('Observer did not fail open: ' + type(error).__name__)
            self.assertIn('run_checks=true', output.read_text())
            self.assertIn('covered=false', output.read_text())

    def test_successful_observation_emits_the_only_positive_marker(self):
        with tempfile.TemporaryDirectory(prefix='zerus-covered-overlap-') as tmp:
            output = Path(tmp) / 'output'
            with patch.dict(os.environ, SOURCE_COMMIT=SHA, GITHUB_OUTPUT=str(output)), \
                    patch.object(overlap, 'candidate_cover', return_value=123), \
                    contextlib.redirect_stdout(io.StringIO()):
                overlap.main()
            self.assertIn('covered=true', output.read_text())
            self.assertIn('run_checks=false', output.read_text())

    def test_downstream_runs_on_failed_observer_or_missing_output(self):
        text = (ROOT / '.github/workflows/ci-regression.yml').read_text().split('  checks:\n', 1)[1]
        expression = text.split('    if: >-\n', 1)[1].split('    uses:', 1)[0].strip()
        expression = ' '.join(expression.split())  # YAML's folded scalar.
        self.assertIn('!cancelled()', expression, 'Observer failure must not inherit the default success() gate')
        expression = expression.replace('&&', ' and ').replace('||', ' or ')
        expression = re.sub(r'!(?!=)', 'not ', expression)
        expression = re.sub(r"(?<!')\bfalse\b(?!')", 'False', expression)
        class Outputs(SimpleNamespace):
            def __getattr__(self, _): return ''  # GitHub's absent-output value.
        for result, covered, cancelled in product(('success', 'failure', 'skipped', 'cancelled'),
                                                  (None, '', 'false', 'true', 'True', 'TrUe', 'true '), (False, True)):
            # GitHub string equality ignores case; the trusted helper itself
            # emits lowercase Booleans. Normalize this tested comparison only.
            outputs = Outputs(**({} if covered is None else dict(covered=covered.casefold())))
            context = dict(cancelled=lambda: cancelled,
                           needs=SimpleNamespace(overlap=SimpleNamespace(result=result, outputs=outputs)),
                           github=SimpleNamespace(event_name='workflow_run', repository='ufna/zerus',
                               event=SimpleNamespace(repository=SimpleNamespace(private=False),
                                   workflow_run=SimpleNamespace(event='push', head_branch='main',
                                       head_repository=SimpleNamespace(full_name='ufna/zerus'), conclusion='success'))),
                           vars=SimpleNamespace(DEV_UPDATES_ENABLED='true'))
            with self.subTest(observer=result, covered=covered, cancelled=cancelled):
                actual = bool(eval(expression, {'__builtins__': {}}, context))
                self.assertEqual(actual, not cancelled and not (result == 'success' and covered is not None and covered.casefold() == 'true'))
                if result == 'success' and covered in ('True', 'TrUe') and not cancelled:
                    self.assertFalse(actual, 'GitHub compares the positive marker without case sensitivity')
            context['github'].event.workflow_run.head_repository.full_name = 'example/zerus'
            self.assertFalse(bool(eval(expression, {'__builtins__': {}}, context)))


if __name__ == '__main__':
    unittest.main()
