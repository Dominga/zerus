#!/usr/bin/env python3
"""A child session starts in its parent's project (zerus-d5twt).

Each launch path runs through the actual CLI: a lifecycle agent in an isolated
tmux with a scripted provider, and DeepSeek and OpenCode against private
fixture services. This test process stands in for the live tracked parent: its
record names this process, so the launching hgs proves it as the caller.
"""
import copy
import fcntl
import hashlib
import http.server
import json
import os
from pathlib import Path
import socket
import subprocess
import sys
import tempfile
import threading
import time
import unittest
import uuid

import test_pause

REPO = Path(__file__).resolve().parents[1]
HGS = Path(os.environ.get('HGS_TEST_BIN', REPO / 'target/debug/hgs')).resolve()
test_pause.HGS = HGS
OPENAPI = json.loads((REPO / 'tests/fixtures/opencode2/openapi-2.0.32.json').read_text())


class Placement:
    """The shared controls. A subclass provides `launch(tag, parent)`, which
    starts a new session from `self.folder` as that parent's child and
    returns its name once its launch finished, and `binding(name)`."""

    def lead(self, tag):
        start = subprocess.check_output(['ps', '-p', str(os.getpid()), '-o', 'lstart='],
                                        env=dict(self.env, LC_ALL='C', TZ='UTC'), text=True).strip()
        record = dict(version=1, name=f'claude/lead/{tag}', agent='claude', run_id=f'run-{tag}',
                      conversation_id=f'conversation-{tag}', session_id=str(uuid.uuid4()), subagents={},
                      pid=os.getpid(), process_start=start)
        path = self.state / (hashlib.sha256(record['name'].encode()).hexdigest() + '.json')
        path.write_text(json.dumps(record))
        self.leads.append(path)
        return record

    def as_child_of(self, parent):
        return dict(self.env, HGS_SESSION=parent['name'], HGS_RUN_ID=parent['run_id'])

    def swarm(self, *args, request=None):
        result = subprocess.run([str(HGS), 'swarm', *args], input=None if request is None else json.dumps(request),
                                env=self.env, capture_output=True, text=True, timeout=30)
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        return json.loads(result.stdout)

    def project_of(self, name):
        record = self.binding(name)
        return self.swarm('session-project', '--json', request={
            'request_id': str(uuid.uuid4()), 'name': name, 'expected_run_id': record['run_id'],
            'expected_conversation_id': record['conversation_id'], 'expected_machine_id': self.node})['current_project_id']

    def explicit(self, name):
        """Whether `name` has a membership of its own in the shared catalog."""
        return any(f'"session":{json.dumps(name)}' in token.replace(' ', '') for token in self.swarm('get')['versions'])

    def catalog(self):
        self.leads = getattr(self, 'leads', [])
        self.node = self.swarm('hello')['node_id']
        parents = {key: self.lead(key) for key in ('work', 'inbox', 'gone')}
        self.swarm('initialize', request={'projects': [
            dict(id='work', name='Work', color='#123456', folders=[], sessions=[self.node + '\n' + parents['work']['name']]),
            # The children's folder belongs to another project.
            dict(id='tree', name='Tree', color='#234567', sessions=[],
                 folders=[dict(id='anchor', machine_id=self.node, path=str(self.folder), name='')]),
            dict(id='other', name='Other', color='#345678', folders=[], sessions=[self.node + '\n' + self.named('explicit')]),
            dict(id='gone', name='Gone', color='#456789', folders=[], sessions=[self.node + '\n' + parents['gone']['name']])]})
        # Another device deletes Gone while its member still names it.
        before = self.swarm('get')
        base = copy.deepcopy(before['organization'])
        for project in base['projects']:
            if project['id'] == 'gone':
                project['sessions'] = []
        desired = copy.deepcopy(before['organization'])
        desired['projects'] = [p for p in desired['projects'] if p['id'] != 'gone']
        self.swarm('apply', request=dict(base=base, desired=desired, versions=before['versions']))
        self.assertTrue(self.explicit(parents['gone']['name']))
        self.assertNotIn('gone', [p['id'] for p in self.swarm('get')['organization']['projects']])
        return parents

    def until(self, predicate, what, within=30):
        ends = time.monotonic() + within
        while not predicate():
            self.assertLess(time.monotonic(), ends, what)
            time.sleep(0.02)

    @staticmethod
    def free(path, within=1.5):
        """Whether the lock at `path` can be taken while the launch waits."""
        with open(path, 'a') as lock:
            until = time.monotonic() + within
            while True:
                try:
                    fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
                    fcntl.flock(lock, fcntl.LOCK_UN)
                    return True
                except BlockingIOError:
                    if time.monotonic() >= until:
                        return False
                    time.sleep(0.01)

    def check_placement(self):
        parents = self.catalog()
        # The parent's live project wins over the folder's.
        placed = self.launch('placed', parents['work'])
        self.assertEqual(self.project_of(placed), 'work')
        # A parent in Inbox decides nothing: the folder's project.
        inferred = self.launch('inferred', parents['inbox'])
        self.assertEqual(self.project_of(inferred), 'tree')
        # A parent whose project was deleted: an explicit Inbox membership.
        orphan = self.launch('orphan', parents['gone'])
        self.assertEqual(self.project_of(orphan), 'ungrouped')
        self.assertTrue(self.explicit(orphan), 'the orphan was not placed in Inbox')
        # An existing explicit membership of the child stays.
        kept = self.launch('explicit', parents['work'])
        self.assertEqual(self.project_of(kept), 'other')
        # A launch without a parent still infers its folder.
        plain = self.launch('plain', None)
        self.assertEqual(self.project_of(plain), 'tree')
        # The parents and existing sessions never move.
        self.assertEqual(self.project_of(placed), 'work')
        self.assertFalse(self.explicit(parents['inbox']['name']))

        catalog_lock = self.config / 'swarm' / 'catalog.lock'
        # A busy catalog: the child waits outside the state and provider
        # locks, then is still placed.
        with open(catalog_lock, 'r+') as catalog:
            fcntl.flock(catalog, fcntl.LOCK_EX)
            try:
                waited = self.launch('waited', parents['work'], busy=lambda: [
                    self.assertTrue(self.free(lock), f'{lock.name} stayed held while the catalog was busy')
                    for lock in self.held_locks()])
            finally:
                fcntl.flock(catalog, fcntl.LOCK_UN)
            self.assertEqual(self.project_of(waited()), 'work')
            # A catalog that stays busy never blocks the launch: the child
            # starts unplaced within the bounded wait.
            fcntl.flock(catalog, fcntl.LOCK_EX)
            try:
                left = self.launch('left', parents['work'])
            finally:
                fcntl.flock(catalog, fcntl.LOCK_UN)
        self.assertFalse(self.explicit(left), 'a busy catalog was waited for without a bound')
        self.assertEqual(self.project_of(left), 'ungrouped')


# Reuse only the lifecycle fixture infrastructure, not its test methods.
Base = type('LifecycleFixture', (unittest.TestCase,), {name: value for name, value in test_pause.Harness.__dict__.items()
    if name not in ('__dict__', '__weakref__') and not name.startswith('test_')})


@unittest.skipIf(os.name == 'nt', 'POSIX native-process ancestry fixture')
class LifecycleChild(Placement, Base):
    def setUp(self):
        super().setUp()
        self.leads = []
        self.state = self.root / 'state'
        self.state.mkdir(exist_ok=True)
        self.config = self.root / 'cfg'
        self.folder = self.project.resolve()

    def tearDown(self):
        # Teardown stops every recorded process; the leads name this test.
        for path in self.leads:
            path.unlink(missing_ok=True)
        super().tearDown()

    def named(self, tag):
        return f'claude/p/{tag}'

    def binding(self, name):
        return Base.binding(self, name)

    def held_locks(self):
        return [self.state / '.lock']

    def launch(self, tag, parent, busy=None):
        name = self.named(tag)
        env = self.env if parent is None else self.as_child_of(parent)
        result = subprocess.run([str(HGS), 'claude', 'p', '-n', tag, '-d', '--new'], env=env,
                                capture_output=True, text=True, timeout=30)
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        path = self.state / (hashlib.sha256(name.encode()).hexdigest() + '.json')
        # The supervisor places the child before its agent starts.
        started = lambda: self.until(lambda: self.binding(name).get('conversation_id'), 'the child never started')
        if busy is not None:
            self.until(path.exists, 'the child binding never appeared')
            time.sleep(0.5)
            busy()
            def finish():
                started()
                return name
            return finish
        started()
        self.assertEqual(self.binding(name).get('parent', {}).get('source'), None if parent is None else 'launch')
        return name

    def test_a_lifecycle_child_starts_in_its_parents_project(self):
        self.check_placement()


@unittest.skipIf(sys.platform == 'win32', 'native providers are supported on Linux/macOS')
class NativeChild(Placement, unittest.TestCase):
    def setUp(self):
        # A short root keeps the dsh host socket under the macOS sun_path limit (104).
        self.temp = tempfile.TemporaryDirectory(prefix='zc-', dir='/tmp')
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name).resolve()
        self.state = self.root / 'state'
        self.state.mkdir()
        self.config = self.root / 'config'
        self.folder = self.root / 'workspace'
        self.folder.mkdir()
        self.leads = []
        self.addCleanup(lambda: [path.unlink(missing_ok=True) for path in self.leads])
        sockets = self.root / 'tmux'
        sockets.mkdir(mode=0o700)
        self.env = {k: v for k, v in os.environ.items() if not k.startswith(
            ('HGS_', 'TMUX', 'XDG_', 'ZERUS_', 'OPENCODE_', 'DSH_'))
            and k not in ('DISPLAY', 'WAYLAND_DISPLAY', 'DBUS_SESSION_BUS_ADDRESS', 'SSH_AUTH_SOCK', 'LD_PRELOAD')}
        self.env.update(HOME=str(self.root), USERPROFILE=str(self.root), HGS_CONFIG_DIR=str(self.config),
                        HGS_STATE_DIR=str(self.state), HGS_SELF='fixture', HGS_PEERS='', TMUX_TMPDIR=str(sockets),
                        XDG_STATE_HOME=str(self.root / 'xdg-state'), XDG_CONFIG_HOME=str(self.root / 'xdg-config'),
                        XDG_DATA_HOME=str(self.root / 'xdg-data'), XDG_CACHE_HOME=str(self.root / 'xdg-cache'))
        (self.state / 'local-sessions').touch()

    def binding(self, name):
        for directory in (self.state / self.agent / 'bindings', self.state / self.agent / 'sessions'):
            for path in directory.glob('*.json'):
                record = json.loads(path.read_text())
                if record.get('name') == name:
                    return record
        return {}

    def named(self, tag):
        return f'{self.agent}/workspace/{tag}'

    def held_locks(self):
        return [self.state / '.lock', self.state / self.agent / 'bindings.lock']

    def launch(self, tag, parent, busy=None):
        name = self.named(tag)
        env = dict(self.env if parent is None else self.as_child_of(parent), **self.agent_env)
        argv = [str(HGS), self.agent, str(self.folder), '-n', tag, '-d', '--new']
        if busy is None:
            result = subprocess.run(argv, env=env, capture_output=True, text=True, timeout=30)
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
            self.assertEqual(self.binding(name).get('parent', {}).get('source'), None if parent is None else 'launch')
            return name
        process = subprocess.Popen(argv, env=env, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
        self.until(lambda: self.binding(name).get('run_id'), 'the child binding never appeared')
        time.sleep(0.5)
        try:
            busy()
        except BaseException:
            process.kill()
            process.communicate()
            raise
        def finish():
            stdout, stderr = process.communicate(timeout=30)
            self.assertEqual(process.returncode, 0, stdout + stderr)
            return name
        return finish

    def test_a_deepseek_child_starts_in_its_parents_project(self):
        self.agent = 'dsh'
        native_home = self.root / 'dsh-home'
        native_home.mkdir()
        host = self.state / 'dsh/hosts' / hashlib.sha256(str(native_home).encode()).hexdigest()[:16]
        host.mkdir(parents=True)
        server = socket.socket(socket.AF_UNIX)
        server.bind(str(host / 'host.sock'))
        server.listen()
        server.settimeout(.1)
        stopped = threading.Event()
        def serve():
            while not stopped.is_set():
                try:
                    client, _ = server.accept()
                except socket.timeout:
                    continue
                with client:
                    line = client.makefile().readline()
                    if line:
                        value = {'version': '0.2.1-rc.2', 'protocol': 1, 'generation': 'private-fixture'}
                        client.sendall((json.dumps({'ok': True, 'value': value}) + '\n').encode())
        thread = threading.Thread(target=serve)
        thread.start()
        def stop():
            stopped.set()
            thread.join(timeout=3)
            server.close()
            self.assertFalse(thread.is_alive())
        self.addCleanup(stop)
        self.agent_env = dict(DSH_HOME=str(native_home))
        self.check_placement()

    def test_an_opencode_child_starts_in_its_parents_project(self):
        self.agent = 'opencode'
        sessions = {}
        class Service(http.server.BaseHTTPRequestHandler):
            def log_message(self, *_):
                pass
            def do_GET(self):
                if self.path == '/api/info':
                    self.reply({'version': '2.0.32'})
                elif self.path == '/openapi.json':
                    self.reply(OPENAPI)
                elif self.path.startswith('/api/session/') and self.path.removeprefix('/api/session/') in sessions:
                    self.reply({'data': sessions[self.path.removeprefix('/api/session/')]})
                else:
                    self.reply({'data': []})
            def do_POST(self):
                value = json.loads(self.rfile.read(int(self.headers['Content-Length'])))
                sessions[value['id']] = value
                self.reply({'data': {'id': value['id'], 'location': value['location']}})
            def reply(self, value):
                body = json.dumps(value).encode()
                self.send_response(200)
                self.send_header('Content-Length', str(len(body)))
                self.end_headers()
                self.wfile.write(body)
        server = http.server.ThreadingHTTPServer(('127.0.0.1', 0), Service)
        worker = threading.Thread(target=server.serve_forever)
        worker.start()
        def stop():
            server.shutdown()
            worker.join(timeout=3)
            server.server_close()
            self.assertFalse(worker.is_alive())
        self.addCleanup(stop)
        registration = self.root / 'xdg-state/opencode/service.json'
        registration.parent.mkdir(parents=True)
        registration.write_text(json.dumps(dict(id='private-fixture', version='2.0.32', pid=os.getpid(),
            url='http://127.0.0.1:' + str(server.server_port), password='fixture-only')))
        native = self.root / 'opencode-config'
        native.mkdir()
        self.agent_env = dict(OPENCODE_CONFIG_DIR=str(native))
        self.check_placement()


if __name__ == '__main__':
    unittest.main()
