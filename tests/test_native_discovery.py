"""Private loopback controls for latest-only native release discovery."""
import contextlib
import io
import subprocess
import ssl
import sys
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import importlib.util
import json
from pathlib import Path
import socketserver
import threading
import time
import unittest
from unittest.mock import call, patch
import urllib.error

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location('discovery_compat_control', ROOT / 'scripts/ci/agent-compatibility.py')
compat = importlib.util.module_from_spec(spec)
spec.loader.exec_module(compat)
discovery = compat.discovery


@contextlib.contextmanager
def server(actions):
    calls, stop = [], threading.Event()

    class Handler(BaseHTTPRequestHandler):
        def do_GET(self):
            index = len(calls)
            calls.append(self.path)
            action = actions[min(index, len(actions) - 1)]
            if action == 'stall_headers':
                stop.wait(15)
                return
            if action in ('stall_body', 'drip_body'):
                self.send_response(200)
                self.send_header('Content-Length', '100')
                self.end_headers()
                self.wfile.write(b'{')
                self.wfile.flush()
                if action == 'drip_body':
                    try:
                        for _ in range(99):
                            if stop.wait(0.05):
                                break
                            self.wfile.write(b'x')
                            self.wfile.flush()
                    except (BrokenPipeError, ConnectionResetError):
                        pass
                stop.wait(15)
                return
            status, body = action
            self.send_response(status)
            self.send_header('Content-Length', str(len(body)))
            self.end_headers()
            try:
                self.wfile.write(body)
            except (BrokenPipeError, ConnectionResetError):
                pass

        def log_message(self, *args):
            pass

    class LoopbackServer(ThreadingHTTPServer):
        def server_bind(self):
            # HTTPServer.server_bind reverse-resolves its host. This fixture
            # needs only a numeric loopback bind, with no resolver dependency.
            socketserver.TCPServer.server_bind(self)
            self.server_name, self.server_port = self.server_address[:2]

    httpd = LoopbackServer(('127.0.0.1', 0), Handler)
    httpd.daemon_threads = True
    thread = threading.Thread(target=lambda: httpd.serve_forever(poll_interval=0.01))
    thread.start()
    try:
        yield 'http://127.0.0.1:' + str(httpd.server_port) + '/latest', calls
    finally:
        stop.set()
        httpd.shutdown()
        httpd.server_close()
        thread.join(2)


class NativeDiscovery(unittest.TestCase):
    @contextlib.contextmanager
    def budget(self, *, attempt=2, total=7):
        with patch.object(discovery, 'ATTEMPT_SECONDS', attempt), \
                patch.object(discovery, 'BUDGET_SECONDS', total), \
                patch.object(discovery, 'BACKOFF_SECONDS', (0.02, 0.04)):
            yield

    @staticmethod
    def diagnostics(text):
        return [json.loads(line.removeprefix('ZERUS_DISCOVERY=')) for line in text.splitlines()
                if line.startswith('ZERUS_DISCOVERY=')]

    def test_loopback_fixture_binds_without_reverse_dns(self):
        with patch('socket.getfqdn', side_effect=AssertionError('loopback fixture must not resolve names')) as reverse:
            with self.budget(), server([(200, b'{"version":"9.8.7"}')]) as (url, calls):
                self.assertEqual(compat.fetch_json(url), {'version': '9.8.7'})
            self.assertEqual(calls, ['/latest'])
        reverse.assert_not_called()

    def test_metadata_retry_resolves_one_response_from_the_same_endpoint(self):
        with self.budget(), server([(503, b'transient'), (200, b'{"version":"9.8.7"}')]) as (url, calls):
            try:
                result = compat.fetch_json(url)
            except discovery.DiscoveryUnavailable as error:
                self.fail('a transient first response must recover: ' + str(error))
            self.assertEqual(result, {'version': '9.8.7'})
        self.assertEqual(calls, ['/latest', '/latest'])

    def test_stalled_headers_and_partial_body_retry_without_mixing_responses(self):
        for stalled in ('stall_headers', 'stall_body', 'drip_body'):
            error = io.StringIO()
            with self.subTest(stalled=stalled), self.budget(), contextlib.redirect_stderr(error), \
                    server([stalled, (200, b'{"version":"9.8.7"}')]) as (url, calls):
                self.assertEqual(compat.fetch_json(url), {'version': '9.8.7'})
            self.assertEqual(calls, ['/latest', '/latest'])
            rows = self.diagnostics(error.getvalue())
            self.assertEqual([r['event'] for r in rows], ['start', 'failed', 'backoff', 'start', 'success'])
            self.assertEqual([r['attempt'] for r in rows if r['event'] == 'start'], [1, 2])
            self.assertTrue(all(r['url'] == url for r in rows))
            self.assertIn('wall timeout', rows[1]['error'])

    def test_exhausted_stalls_fail_closed_with_an_attributable_unavailable_result(self):
        error = io.StringIO()
        started = time.monotonic()
        with self.budget(), contextlib.redirect_stderr(error), server(['stall_headers']) as (url, calls):
            ready = time.monotonic()
            with self.assertRaisesRegex(discovery.DiscoveryUnavailable, 'discovery unavailable:.*3 attempt'):
                compat.fetch_json(url)
            exhausted = time.monotonic()
        finished = time.monotonic()
        self.assertEqual(calls, ['/latest'] * 3)
        self.assertLess(finished - started, 11, 'phase seconds: ' + json.dumps(dict(
            fixture_setup=ready - started, discovery=exhausted - ready, fixture_cleanup=finished - exhausted)))
        rows = self.diagnostics(error.getvalue())
        self.assertEqual([r['attempt'] for r in rows if r['event'] == 'failed'], [1, 2, 3])
        self.assertEqual(rows[-1]['event'], 'unavailable')
        self.assertNotIn('skipped', error.getvalue())

    def test_original_deadline_bounds_the_request_and_is_not_renewed_by_retry(self):
        error = io.StringIO()
        with self.budget(), contextlib.redirect_stderr(error), server(['stall_body']) as (url, calls):
            started = time.monotonic()
            with self.assertRaises(discovery.DiscoveryUnavailable):
                discovery.fetch_metadata(url, 100, deadline=started + 0.5)
        self.assertLess(time.monotonic() - started, 3)
        # A loaded runner may spend the tiny test budget starting Python; it must
        # still cancel the request and must never renew the original deadline.
        self.assertLessEqual(len(calls), 1)
        self.assertTrue(all(path == '/latest' for path in calls))
        self.assertEqual([r['attempt'] for r in self.diagnostics(error.getvalue())
                          if r['event'] == 'start'], [1])

    def test_expired_deadline_makes_no_request(self):
        with self.budget(), server([(200, b'{}')]) as (url, calls):
            with self.assertRaises(discovery.DiscoveryUnavailable):
                discovery.fetch_metadata(url, 100, deadline=time.monotonic() - 1)
        self.assertEqual(calls, [])

    def test_failed_discovery_exits_nonzero_before_the_native_marker(self):
        code = '''import importlib.util, sys
spec = importlib.util.spec_from_file_location('discovery_exit', sys.argv[1])
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)
module.BACKOFF_SECONDS = (0.01, 0.02)
module.fetch_metadata(sys.argv[2], 100)
print('ZERUS_NATIVE_RESULT={"tests": 1, "skipped": 0}')
'''
        with server([(503, b'busy')]) as (url, calls):
            result = subprocess.run([sys.executable, '-I', '-B', '-c', code, discovery.__file__, url],
                                    capture_output=True, text=True, timeout=15)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('discovery unavailable', result.stderr)
        self.assertNotIn('ZERUS_NATIVE_RESULT', result.stdout)
        self.assertEqual(calls, ['/latest'] * 3)

    def test_worker_start_failure_is_attributable_and_never_native_success(self):
        with patch.object(discovery.subprocess, 'run', side_effect=OSError('synthetic spawn refusal')) as spawn:
            with self.assertRaisesRegex(discovery.DiscoveryUnavailable, 'metadata worker could not start'):
                discovery.fetch_metadata('https://registry.npmjs.org/example/latest', 100)
        spawn.assert_called_once()

    def test_a_late_success_is_refused_under_the_original_budget(self):
        clock = [0.0]
        def late(*args, **kwargs):
            clock[0] = 5.0
            return subprocess.CompletedProcess([], 0, b'{"body":"e30=","record":{}}', b'')
        with patch.object(discovery.time, 'monotonic', side_effect=lambda: clock[0]), \
                patch.object(discovery.subprocess, 'run', side_effect=late), \
                patch.object(discovery.time, 'sleep'):
            with self.assertRaises(discovery.DiscoveryUnavailable):
                discovery.fetch_metadata('https://registry.npmjs.org/example/latest', 100, deadline=4)

    def test_backoff_actually_waits_and_respects_the_original_deadline(self):
        failure = subprocess.CompletedProcess([], 1, json.dumps(dict(
            error='transient', error_type='TimeoutError', retryable=True)).encode(), b'')
        success = subprocess.CompletedProcess([], 0, b'{"body":"e30=","record":{}}', b'')
        with patch.object(discovery.subprocess, 'run', side_effect=[failure, failure, success]), \
                patch.object(discovery.time, 'sleep') as sleep:
            self.assertEqual(discovery.fetch_metadata('https://registry.npmjs.org/example/latest', 100)[0], b'{}')
        self.assertEqual(sleep.call_args_list, [call(2), call(4)])
        clock = [0.0]
        def wait(seconds):
            clock[0] += seconds
        with patch.object(discovery.subprocess, 'run', return_value=failure) as run, \
                patch.object(discovery.time, 'monotonic', side_effect=lambda: clock[0]), \
                patch.object(discovery.time, 'sleep', side_effect=wait) as sleep:
            with self.assertRaises(discovery.DiscoveryUnavailable):
                discovery.fetch_metadata('https://registry.npmjs.org/example/latest', 100, deadline=1)
        run.assert_called_once()
        sleep.assert_called_once_with(1)

    def test_certificate_failure_is_permanent_and_worker_exits_nonzero(self):
        error = urllib.error.URLError(ssl.SSLCertVerificationError('synthetic invalid certificate'))
        output = io.StringIO()
        with patch.object(discovery, 'read_metadata', side_effect=error), \
                patch.object(discovery.sys, 'argv', ['reader', '--read', 'https://example.test/latest', '100', 'GET', 'npm']), \
                contextlib.redirect_stdout(output):
            self.assertEqual(discovery.worker(), 1)
        result = json.loads(output.getvalue())
        self.assertFalse(result['retryable'])
        self.assertIn('synthetic invalid certificate', result['error'])

    def test_permanent_http_failure_and_oversize_are_not_retried(self):
        for action, limit in [((404, b'missing'), 100), ((200, b'x' * 101), 100)]:
            with self.subTest(action=action), self.budget(), server([action]) as (url, calls):
                with self.assertRaisesRegex(ValueError, 'discovery invalid'):
                    discovery.fetch_metadata(url, limit)
            self.assertEqual(calls, ['/latest'])

    def test_malformed_json_is_not_retried_or_replaced_with_an_old_version(self):
        with self.budget(), server([(200, b'{'), (200, b'{"version":"old"}')]) as (url, calls):
            with self.assertRaises(json.JSONDecodeError):
                compat.fetch_json(url)
        self.assertEqual(calls, ['/latest'])

    def test_retry_429_and_503_with_backoff_but_never_more_than_three_attempts(self):
        error = io.StringIO()
        with self.budget(), contextlib.redirect_stderr(error), \
                server([(429, b'busy'), (503, b'busy'), (200, b'latest')]) as (url, calls):
            raw, record = discovery.fetch_metadata(url, 100)
        self.assertEqual(raw, b'latest')
        self.assertEqual(record['attempts'], 3)
        self.assertEqual(calls, ['/latest'] * 3)
        self.assertEqual([r['seconds'] for r in self.diagnostics(error.getvalue())
                          if r['event'] == 'backoff'], [0.02, 0.04])

    def test_every_discovery_reader_uses_the_same_helper_and_preserves_its_policy(self):
        modules = [compat]
        for name in ('kimi-native', 'dsh_fixture'):
            spec = importlib.util.spec_from_file_location('control_' + name, ROOT / 'scripts/ci' / (name + '.py'))
            module = importlib.util.module_from_spec(spec)
            spec.loader.exec_module(module)
            modules.append(module)
        kimi, dsh = modules[1:]
        with patch.object(compat.discovery, 'fetch_metadata', return_value=(b'{"version":"9.8.7"}', {})) as fetch:
            self.assertEqual(compat.fetch_json('https://registry.npmjs.org/example/latest'), {'version': '9.8.7'})
            fetch.assert_called_once_with('https://registry.npmjs.org/example/latest', 8 * 1024**2,
                                          stage='official npm metadata')
        with patch.object(kimi.discovery, 'fetch_metadata', return_value=(b'9.8.7', {})) as fetch:
            self.assertEqual(kimi.metadata('https://code.kimi.com/kimi-code/latest', 64), (b'9.8.7', {}))
            fetch.assert_called_once_with('https://code.kimi.com/kimi-code/latest', 64,
                                          method='GET', policy='kimi', stage='official Kimi metadata')
        end = time.monotonic() + 10
        with patch.object(dsh.discovery, 'fetch_metadata', return_value=(b'{}', {})) as fetch:
            self.assertEqual(dsh.discover(end), b'{}')
            fetch.assert_called_once_with(dsh.LATEST, dsh.MAX_METADATA, policy='no-redirect',
                                          deadline=end, stage='official DeepSeek latest')

    def test_real_kimi_and_deepseek_readers_recover_at_the_same_endpoint(self):
        for name in ('kimi-native', 'dsh_fixture'):
            spec = importlib.util.spec_from_file_location('recovery_' + name, ROOT / 'scripts/ci' / (name + '.py'))
            module = importlib.util.module_from_spec(spec)
            spec.loader.exec_module(module)
            with self.subTest(reader=name), patch.object(module.discovery, 'BACKOFF_SECONDS', (0.02, 0.04)), \
                    server([(503, b'busy'), (200, b'9.8.7')]) as (url, calls):
                if name == 'kimi-native':
                    self.assertEqual(module.metadata(url, 64)[0], b'9.8.7')
                else:
                    with patch.object(module, 'LATEST', url):
                        self.assertEqual(module.discover(time.monotonic() + 10), b'9.8.7')
            self.assertEqual(calls, ['/latest', '/latest'])
