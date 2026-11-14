"""Bounded retries of one metadata URL; no package selection or execution.

Each request runs in a disposable child so DNS, redirects, headers and a slowly
dripping body share the same wall timeout. Killing that child cannot leave a
request thread alive in the caller. Package bytes/integrity are handled elsewhere.
"""
import base64
import http.client
import json
from pathlib import Path
import ssl
import subprocess
import sys
import time
import urllib.error
import urllib.parse
import urllib.request

ATTEMPTS = 3
ATTEMPT_SECONDS = 30
BACKOFF_SECONDS = (2, 4)
BUDGET_SECONDS = ATTEMPTS * ATTEMPT_SECONDS + sum(BACKOFF_SECONDS)
MAX_METADATA = 8 * 1024**2


class DiscoveryUnavailable(ValueError):
    """Discovery failed closed, before any native suite can be admitted."""


class NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        raise ValueError('Official npm discovery must not redirect')


class OfficialRedirect(urllib.request.HTTPRedirectHandler):
    def __init__(self, method='GET'):
        self.hops = []
        self.method = method

    def redirect_request(self, request, fp, code, message, headers, newurl):
        source, destination = map(urllib.parse.urlsplit, (request.full_url, newurl))
        if (self.hops or code != 302 or len(headers.get_all('Location', [])) != 1
                or request.get_method() != self.method or self.method not in ('GET', 'HEAD')
                or source.scheme != 'https' or source.netloc != 'code.kimi.com'
                or not (source.path.startswith('/kimi-code/binaries/') or source.path == '/kimi-code/latest')
                or source.query or source.fragment
                or destination.scheme != 'https' or destination.netloc != 'cdn.kimi.com'
                or destination.path != source.path or destination.query or destination.fragment):
            raise ValueError('unreviewed download redirect')
        self.hops.append(dict(source=request.full_url, target=newurl, status=code))
        return urllib.request.Request(newurl, headers=dict(request.headers), method=self.method)


def read_metadata(url, limit, method='GET', policy='npm'):
    """One child request, retaining each caller's redirect and byte validation."""
    redirects = OfficialRedirect(method)
    headers = {'Accept-Encoding': 'identity'}
    if policy == 'npm':
        headers['User-Agent'] = 'Zerus-agent-compatibility'
    elif policy == 'no-redirect':
        headers.update({'User-Agent': 'Zerus-DeepSeek-CI', 'Accept': 'application/json'})
    request = urllib.request.Request(url, headers=headers, method=method)
    if policy == 'kimi':
        opener = urllib.request.build_opener(urllib.request.ProxyHandler({}), redirects)
    elif policy == 'no-redirect':
        opener = urllib.request.build_opener(NoRedirect)
    else:
        opener = urllib.request.build_opener()
    with opener.open(request, timeout=ATTEMPT_SECONDS) as response:
        if policy == 'kimi':
            lengths = response.headers.get_all('Content-Length', [])
            if (response.status != 200 or len(lengths) != 1 or not lengths[0].isascii()
                    or not lengths[0].isdigit() or not 0 < int(lengths[0]) <= limit
                    or response.headers.get('Content-Encoding', 'identity') != 'identity'):
                raise ValueError('official metadata status/length/encoding is invalid')
            size, body = int(lengths[0]), bytearray()
            if method == 'GET':
                while len(body) < size:
                    chunk = response.read(min(65536, size - len(body)))
                    if not chunk:
                        raise ValueError('short metadata')
                    body.extend(chunk)
                if response.read(1):
                    raise ValueError('oversized metadata')
        else:
            body = response.read(limit + 1)
            if len(body) > limit:
                raise ValueError('Upstream metadata exceeds its size limit')
            size = len(body)
        return bytes(body), dict(url=url, final_url=response.url, redirects=redirects.hops, size=size)


def diagnostic(**fields):
    print('ZERUS_DISCOVERY=' + json.dumps(fields, sort_keys=True), file=sys.stderr, flush=True)


def fetch_metadata(url, limit, *, method='GET', policy='npm', deadline=None, stage='official metadata'):
    """At most 3 x 30s plus 2s/4s backoff (96s), clamped to an original deadline.

    A retry uses the identical URL/method/policy. Version and integrity validation
    happen once after success; neither retained versions nor native tests run here.
    """
    if (method not in ('GET', 'HEAD') or policy not in ('npm', 'no-redirect', 'kimi')
            or not 0 < limit <= (512 * 1024**2 if method == 'HEAD' else MAX_METADATA)):
        raise ValueError('Invalid bounded discovery request')
    end = time.monotonic() + BUDGET_SECONDS
    if deadline is not None:
        end = min(end, deadline)
    common = dict(url=url, method=method, stage=stage, max_attempts=ATTEMPTS,
                  budget_seconds=BUDGET_SECONDS)
    last, attempts = 'original discovery deadline expired', 0
    for attempt in range(1, ATTEMPTS + 1):
        remaining = min(ATTEMPT_SECONDS, end - time.monotonic())
        if remaining <= 0:
            break
        attempts = attempt
        attempt_end = min(end, time.monotonic() + remaining)
        diagnostic(**common, attempt=attempt, event='start', timeout_seconds=remaining)
        try:
            result = subprocess.run([sys.executable, '-I', '-B', str(Path(__file__).resolve()),
                                     '--read', url, str(limit), method, policy],
                                    capture_output=True, timeout=remaining, check=False)
        except subprocess.TimeoutExpired:
            # subprocess.run kills AND reaps its original child on timeout.
            failure = dict(error='request wall timeout', error_type='TimeoutError', retryable=True)
        except OSError as error:
            diagnostic(**common, attempt=attempt, event='unavailable', error='metadata worker: ' + str(error)[:512])
            raise DiscoveryUnavailable('discovery unavailable: metadata worker could not start: '
                                       + stage + ': ' + url) from error
        else:
            try:
                value = json.loads(result.stdout)
            except (ValueError, UnicodeError):
                value = dict(error='metadata worker returned no valid result',
                             error_type='WorkerFailure', retryable=False)
            if time.monotonic() >= attempt_end:
                value = dict(error='request completed after its deadline', error_type='TimeoutError', retryable=True)
            elif result.returncode == 0:
                body = base64.b64decode(value['body'], validate=True)
                record = value['record']
                record['attempts'] = attempt
                diagnostic(**common, attempt=attempt, event='success')
                return body, record
            failure = value
        last = failure['error_type'] + ': ' + failure['error']
        if not failure['retryable']:
            diagnostic(**common, attempt=attempt, event='invalid', error=last)
            raise ValueError('discovery invalid: ' + stage + ': ' + url + ': ' + last)
        diagnostic(**common, attempt=attempt, event='failed', error=last)
        if attempt < ATTEMPTS:
            delay = min(BACKOFF_SECONDS[attempt - 1], max(0, end - time.monotonic()))
            diagnostic(**common, attempt=attempt, event='backoff', seconds=delay)
            time.sleep(delay)
    diagnostic(**common, attempt=attempts, event='unavailable', error=last)
    raise DiscoveryUnavailable('discovery unavailable: ' + stage + ': ' + url
                               + f' after {attempts} attempt(s): ' + last)


def worker():
    """Internal byte reader; its nonzero result is never a skipped-suite success."""
    try:
        if len(sys.argv) != 6 or sys.argv[1] != '--read':
            raise ValueError('Only the fixed metadata reader is supported')
        _, _, url, limit, method, policy = sys.argv
        body, record = read_metadata(url, int(limit), method, policy)
        print(json.dumps(dict(body=base64.b64encode(body).decode('ascii'), record=record)))
        return 0
    except Exception as error:
        retryable = isinstance(error, (OSError, http.client.HTTPException))
        if isinstance(error, urllib.error.HTTPError):
            retryable = error.code in (408, 429) or 500 <= error.code < 600
            error.close()
        reason = error.reason if isinstance(error, urllib.error.URLError) else error
        if isinstance(reason, (ssl.SSLCertVerificationError, ValueError)):
            retryable = False
        print(json.dumps(dict(error_type=type(error).__name__, error=str(error)[:512], retryable=retryable)))
        return 1


if __name__ == '__main__':
    raise SystemExit(worker())
