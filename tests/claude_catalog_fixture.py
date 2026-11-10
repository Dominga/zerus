"""Private served-catalog context for the real Claude binary.

The native first-party catalog has expandable model sections. A loopback TLS
proxy supplies that context with fixture credentials; it never forwards traffic.
Its temporary CA is trusted only by the selected native fixture process.
"""
import copy
import hashlib
import http.server
import json
from pathlib import Path
import ssl
import subprocess
import threading
import time


ACCOUNT = '11111111-2222-4333-8444-555555555555'
ORGANIZATION = '66666666-7777-4888-8999-aaaaaaaaaaaa'


class CatalogFixture:
    def __init__(self, root, catalog):
        self.root = Path(root) / 'catalog-proxy'
        self.root.mkdir(mode=0o700)
        self.catalog = copy.deepcopy(catalog['catalog'])
        self.catalog['state'] = dict(id='cc', model='claude-opus-5-5')
        self.requests = []
        config = self.root / 'openssl.cnf'
        config.write_text('[req]\nprompt=no\ndistinguished_name=dn\nx509_extensions=extensions\n'
                          '[dn]\nCN=api.anthropic.com\n[extensions]\n'
                          'subjectAltName=DNS:api.anthropic.com\nbasicConstraints=critical,CA:TRUE\n')
        self.certificate = self.root / 'certificate.pem'
        key = self.root / 'key.pem'
        subprocess.run(['openssl', 'req', '-x509', '-newkey', 'rsa:2048', '-nodes',
                        '-keyout', str(key), '-out', str(self.certificate), '-days', '1',
                        '-config', str(config)], check=True, capture_output=True, timeout=20)
        context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
        context.load_cert_chain(self.certificate, key)
        fixture = self

        class Proxy(http.server.BaseHTTPRequestHandler):
            protocol_version = 'HTTP/1.1'

            def log_message(self, *_args):
                pass

            def do_CONNECT(self):
                if self.path != 'api.anthropic.com:443':
                    self.send_error(403, 'Fixture does not forward traffic')
                    return
                self.send_response(200, 'Connection established')
                self.end_headers()
                self.connection = context.wrap_socket(self.connection, server_side=True)
                self.connection.settimeout(5)
                self.rfile = self.connection.makefile('rb', self.rbufsize)
                self.wfile = self.connection.makefile('wb', self.wbufsize)

            def do_GET(self):
                if len(fixture.requests) >= 128:
                    self.send_error(500, 'Fixture request bound exceeded')
                    return
                fixture.requests.append(('GET', self.path))
                value = {}
                if self.path == '/api/claude_code/settings':
                    value = dict(uuid='22222222-3333-4444-8555-666666666666',
                                 checksum=hashlib.sha256(b'{}').hexdigest(), settings={})
                elif self.path == '/api/claude_code/policy_limits':
                    value = dict(restrictions={})
                elif '/model_selector/' in self.path:
                    value = dict(model_selector_config=[fixture.catalog['config']],
                                 model_selector_state=[fixture.catalog['state']])
                body = json.dumps(value).encode()
                self.send_response(200)
                self.send_header('Content-Type', 'application/json')
                self.send_header('Content-Length', str(len(body)))
                self.end_headers()
                self.wfile.write(body)

            def do_POST(self):
                if len(fixture.requests) < 128:
                    fixture.requests.append(('POST', self.path))
                self.close_connection = True
                self.send_error(500, 'Settings fixture serves no inference requests')

        self.server = http.server.ThreadingHTTPServer(('127.0.0.1', 0), Proxy)
        self.server.daemon_threads = True
        self.thread = threading.Thread(target=self.server.serve_forever, daemon=True)
        self.thread.start()

    def seed(self, home):
        home = Path(home)
        config = home / '.claude.json'
        value = json.loads(config.read_text())
        value['cachedGrowthBookFeatures'] = {
            'tengu_delegated_quail': dict(served='primary', published='off')}
        value['oauthAccount'] = dict(accountUuid=ACCOUNT, organizationUuid=ORGANIZATION,
                                    subscriptionType='max', billingType='stripe')
        config.write_text(json.dumps(value))
        at = int(time.time() * 1000)
        path = (home / '.claude/cache/model-catalog' /
                (ORGANIZATION + '-' + hashlib.sha256(ACCOUNT.encode()).hexdigest()[:12] + '-cc.json'))
        path.write_text(json.dumps(dict(version=2, fetchedAt=at, staleAt=at + 3600000,
                                        catalog=self.catalog)))

    def environment(self):
        proxy = 'http://127.0.0.1:' + str(self.server.server_port)
        return dict(HTTPS_PROXY=proxy, HTTP_PROXY=proxy, ALL_PROXY=proxy,
                    NO_PROXY='127.0.0.1,localhost', NODE_EXTRA_CA_CERTS=str(self.certificate),
                    DISABLE_TELEMETRY='1', DISABLE_ERROR_REPORTING='1',
                    CLAUDE_CODE_GB_DISK_CACHE_WHEN_TELEMETRY_OFF='1',
                    CLAUDE_CODE_OAUTH_TOKEN='sk-ant-oat01-fixture',
                    CLAUDE_CODE_ACCOUNT_UUID=ACCOUNT,
                    CLAUDE_CODE_ORGANIZATION_UUID=ORGANIZATION)

    def close(self):
        self.server.shutdown()
        self.server.server_close()
        self.thread.join(timeout=2)
