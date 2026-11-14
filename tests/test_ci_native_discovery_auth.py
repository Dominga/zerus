"""Recorded HTTP controls with normalized reset times; no network or native agents."""
import contextlib
from email.message import Message
import importlib.util
import io
import json
import os
from pathlib import Path
import re
import subprocess
import unittest
from unittest.mock import patch, call
import urllib.error
import urllib.request

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location('kimi_auth_control', ROOT / 'scripts/ci/kimi-native.py')
kimi = importlib.util.module_from_spec(spec)
spec.loader.exec_module(kimi)
discovery = kimi.discovery
RECORDED = json.loads((ROOT / 'tests/fixtures/github-native-discovery.json').read_text())
URL = RECORDED['url']
CREDENTIAL = 'synthetic-discovery-credential'


def headers(values):
    result = Message()
    for key, value in values:
        result[key] = str(value)
    return result


def http_error(status, fields, message='recorded HTTP failure'):
    return urllib.error.HTTPError(URL, status, message, headers(fields), io.BytesIO(b'not metadata'))


class Response(io.BytesIO):
    status = 200

    def __init__(self, url, body):
        super().__init__(body)
        self.url = url
        self.headers = headers([('Content-Length', len(body))])


class GitHubDiscovery(unittest.TestCase):
    def worker_failure(self, status, fields, message='recorded HTTP failure'):
        output = io.StringIO()
        with patch.object(discovery, 'read_metadata', side_effect=http_error(status, fields, message)), \
                patch.object(discovery.sys, 'argv', ['reader', '--read', URL, '262144', 'GET', 'no-redirect']), \
                patch.object(discovery.time, 'time', return_value=RECORDED['epoch']), \
                contextlib.redirect_stdout(output):
            self.assertEqual(discovery.worker(), 1)
        return json.loads(output.getvalue())

    def test_token_prevents_the_recorded_unauthenticated_403_without_changing_selection(self):
        requests, commands = [], []

        def open_recorded(request, **kwargs):
            requests.append(request)
            if request.full_url == URL:
                if request.get_header('Authorization') != 'Bearer ' + CREDENTIAL:
                    row = RECORDED['rate_limited']
                    raise http_error(row['status'], row['headers'].items(), row['reason'])
                body = json.dumps(RECORDED['release']).encode()
            elif request.full_url == kimi.NPM_LATEST:
                body = json.dumps(dict(name=kimi.PACKAGE, version='2.6.2')).encode()
            else:
                self.assertEqual(request.full_url, 'https://code.kimi.com/kimi-code/latest')
                body = b'2.5.3'
            return Response(request.full_url, body)

        def worker_process(argv, **kwargs):
            commands.append(argv)
            output = io.StringIO()
            with patch.object(discovery.sys, 'argv', argv[3:]), contextlib.redirect_stdout(output):
                code = discovery.worker()
            return subprocess.CompletedProcess(argv, code, output.getvalue().encode(), b'')

        diagnostics = io.StringIO()
        with patch.dict(os.environ, GITHUB_TOKEN=CREDENTIAL), \
                patch.object(discovery.urllib.request, 'build_opener') as build, \
                patch.object(discovery.subprocess, 'run', side_effect=worker_process), \
                contextlib.redirect_stderr(diagnostics):
            build.return_value.open.side_effect = open_recorded
            version, selection = kimi.current_release()
        self.assertEqual(version, '2.6.2')
        self.assertEqual(selection['agreeing_sources'], ['npm', 'github'])
        self.assertEqual([r.full_url for r in requests], [
            'https://code.kimi.com/kimi-code/latest', kimi.NPM_LATEST, URL])
        self.assertEqual([r.get_header('Authorization') for r in requests], [None, None, 'Bearer ' + CREDENTIAL])
        handler = build.call_args_list[-1].args[0]
        handler = handler() if isinstance(handler, type) else handler
        with self.assertRaises(ValueError):
            handler.redirect_request(requests[-1], None, 302, '',
                headers([('Location', 'https://example.test/capture')]), 'https://example.test/capture')
        self.assertNotIn(CREDENTIAL, diagnostics.getvalue() + json.dumps(selection) + repr(commands))

    def test_token_is_not_sent_to_other_origins_and_github_must_not_redirect(self):
        urls = ['http://api.github.com/repos/example/latest', 'https://api.github.com.example.test/latest',
                'https://api.github.com:443/latest', 'https://registry.npmjs.org/example/latest',
                'https://code.kimi.com/kimi-code/latest']
        with patch.dict(os.environ, GITHUB_TOKEN=CREDENTIAL):
            for url in urls:
                with self.subTest(url=url), patch.object(discovery.urllib.request, 'build_opener') as build:
                    build.return_value.open.return_value = Response(url, b'{}')
                    discovery.read_metadata(url, 100, policy='no-redirect')
                    self.assertIsNone(build.return_value.open.call_args.args[0].get_header('Authorization'))
            with self.assertRaisesRegex(ValueError, 'redirect'), \
                    patch.object(discovery.urllib.request, 'build_opener') as build:
                discovery.read_metadata(URL, 100, policy='npm')
            build.assert_not_called()
            for method in ('POST', 'PUT'):
                with self.subTest(method=method), self.assertRaisesRegex(ValueError, 'read-only'), \
                        patch.object(discovery.urllib.request, 'build_opener') as build:
                    discovery.read_metadata(URL, 100, method=method, policy='no-redirect')
                build.assert_not_called()
        request = urllib.request.Request(URL)
        with self.assertRaises(ValueError):
            discovery.NoRedirect().redirect_request(request, None, 302, '',
                headers([('Location', 'https://example.test/capture')]), 'https://example.test/capture')

    def test_absent_token_still_allows_public_discovery(self):
        with patch.dict(os.environ, {}, clear=True), \
                patch.object(discovery.urllib.request, 'build_opener') as build:
            build.return_value.open.return_value = Response(URL, b'{}')
            self.assertEqual(discovery.read_metadata(URL, 100, policy='no-redirect')[0], b'{}')
            self.assertIsNone(build.return_value.open.call_args.args[0].get_header('Authorization'))

    def test_only_rate_limits_with_an_unambiguous_reset_are_retryable(self):
        cases = [(403, list(RECORDED['rate_limited']['headers'].items()), 6),
                 (403, [('Retry-After', '4')], 4), (429, [('Retry-After', '3')], 3),
                 (429, [('Retry-After', '2'), ('X-RateLimit-Remaining', '0'),
                        ('X-RateLimit-Reset', '1700000005')], 6)]
        for status, fields, wait in cases:
            with self.subTest(status=status, fields=fields):
                result = self.worker_failure(status, fields)
                self.assertTrue(result['retryable'])
                self.assertEqual(result['retry_after'], wait)
        invalid = [(403, []), (429, []), (401, [('Retry-After', '1')]), (503, [('Retry-After', '1')]),
                   (403, [('X-RateLimit-Remaining', '1'), ('X-RateLimit-Reset', '1700000005')]),
                   (403, [('X-RateLimit-Remaining', '0')]), (429, [('Retry-After', '-1')]),
                   (429, [('Retry-After', 'nan')]), (429, [('Retry-After', '1'), ('Retry-After', '2')]),
                   (403, [('X-RateLimit-Remaining', '0'), ('X-RateLimit-Reset', 'not a reset')])]
        for status, fields in invalid:
            with self.subTest(status=status, fields=fields):
                self.assertFalse(self.worker_failure(status, fields)['retryable'])

    def test_rate_wait_uses_the_same_url_and_never_renews_the_original_budget(self):
        row = RECORDED['rate_limited']
        failure = subprocess.CompletedProcess([], 1, json.dumps(
            self.worker_failure(row['status'], row['headers'].items())).encode(), b'')
        success = subprocess.CompletedProcess([], 0, b'{"body":"e30=","record":{}}', b'')
        for budget, succeeds in [(10, True), (5, False)]:
            clock = [0.0]
            def wait(seconds):
                clock[0] += seconds
            with self.subTest(budget=budget), patch.object(discovery.time, 'monotonic', side_effect=lambda: clock[0]), \
                    patch.object(discovery.time, 'sleep', side_effect=wait) as sleep, \
                    patch.object(discovery.subprocess, 'run', side_effect=[failure, success]) as run:
                if succeeds:
                    self.assertEqual(discovery.fetch_metadata(URL, 100, policy='no-redirect', deadline=budget)[0], b'{}')
                    self.assertEqual(sleep.call_args_list, [call(6)])
                    self.assertEqual(run.call_count, 2)
                    self.assertEqual(run.call_args_list[0].args, run.call_args_list[1].args)
                else:
                    with self.assertRaises(discovery.DiscoveryUnavailable):
                        discovery.fetch_metadata(URL, 100, policy='no-redirect', deadline=budget)
                    run.assert_called_once()
                    sleep.assert_not_called()

    def test_worker_never_serializes_the_credential_even_in_an_exception(self):
        with patch.dict(os.environ, GITHUB_TOKEN=CREDENTIAL):
            result = self.worker_failure(403, [], 'upstream echoed ' + CREDENTIAL)
        self.assertNotIn(CREDENTIAL, json.dumps(result))

    def test_all_kimi_workflows_pass_only_the_readonly_token_at_the_discovery_step(self):
        found = 0
        for name in ('kimi-native.yml', 'kimi-approvals.yml', 'teams-native.yml'):
            text = (ROOT / '.github/workflows' / name).read_text()
            self.assertIn('permissions:\n  contents: read', text)
            self.assertNotIn('secrets.', text)
            for step in re.split(r'(?m)^      - ', text)[1:]:
                if not re.search(r'python -s -B scripts/ci/kimi-(native|approvals)\.py --', step):
                    continue
                found += 1
                self.assertIn('        env:\n          GITHUB_TOKEN: ${{ github.token }}\n', step)
            self.assertEqual(text.count('GITHUB_TOKEN:'), len(re.findall(
                r'python -s -B scripts/ci/kimi-(?:native|approvals)\.py --', text)))
        self.assertEqual(found, 4)
