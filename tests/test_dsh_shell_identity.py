"""zerus-ppd26: a resident DeepSeek host's tool calls carry their own session's
markers, never those of the session that started the host, and never pass as
the person. Released official harness with the keyless fixture model; private
state, configuration, home, tmux and native home; never the user's host.
"""
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import shutil
import shlex
import signal
import socket
import subprocess
import sys
import tempfile
import time
import unittest
import uuid

REPO = Path(__file__).resolve().parents[1]
DSH = shutil.which('dsh')
HGS = Path(os.environ.get('HGS_TEST_BIN', REPO / 'target/debug/hgs'))
_fixture_spec = importlib.util.spec_from_file_location('ci_dsh_fixture', REPO / 'scripts/ci/dsh_fixture.py')
fixture = importlib.util.module_from_spec(_fixture_spec)
_fixture_spec.loader.exec_module(fixture)

# What hgs's host start removes (`host::SESSION_CONTEXT` plus the terminal name).
SESSION_CONTEXT = ['CLAUDECODE', 'HGS_PARENT', 'TMUX', 'TMUX_PANE', 'HGS_HOST_SESSION', 'HGS_HOST_PANE',
                   'HGS_SESSION', 'HGS_SESSION_ID', 'HGS_LAUNCH_SESSION_ID', 'HGS_RESUME_SESSION_ID', 'HGS_AGENT',
                   'HGS_RUN_ID', 'HGS_EXPECTED_ID', 'HGS_REQUESTED_ID', 'HGS_ARCHIVE_ID', 'HGS_FRESH',
                   'HGS_FRESH_REPLACES', 'HGS_FORK_PARENT_ID', 'HGS_FORK_SOURCE_NAME',
                   'HGS_FORK_AGENT_HINT_REVISION', 'HGS_LAUNCH_ID', 'HGS_ACCOUNT_ID', 'HGS_TRACKING', 'HGS_CLIENT',
                   'HGS_TERMINAL_NAME']
SESSION_MARKERS = ['HGS_SESSION', 'HGS_RUN_ID', 'HGS_SESSION_ID', 'HGS_ACCOUNT_ID']

# Runs inside a hosted tool call: records its environment and what the listed
# hgs commands answer there.
PROBE = r'''
import json, os, subprocess, sys, time
cfg = json.load(open(sys.argv[1]))
time.sleep(cfg.get('sleep', 0))
out = {'env': dict(os.environ), 'runs': {}}
for name, argv, stdin, *changes in cfg.get('runs', []):
    env = dict(os.environ)
    for key, value in (changes[0] if changes else {}).items():
        if value is None:
            env.pop(key, None)
        else:
            env[key] = value
    r = subprocess.run(argv, input=stdin, text=True, capture_output=True, timeout=60, env=env)
    out['runs'][name] = {'code': r.returncode, 'stdout': r.stdout[-4000:], 'stderr': r.stderr[-4000:]}
with open(cfg['out'] + '.tmp', 'w') as f:
    json.dump(out, f)
os.rename(cfg['out'] + '.tmp', cfg['out'])
'''

# Run inside a hosted tool call on Linux: the socket inodes of host.sock (its
# listener and any accepted connection), which of them each process of this
# call holds, which the host holds, and whether the host's are close-on-exec.
FD_PROBE = r'''
import json, os, sys
path, host = sys.argv[1], int(sys.argv[2])
inodes = set()
with open('/proc/net/unix') as table:
    next(table)
    for line in table:
        fields = line.split()
        if len(fields) >= 8 and fields[7] == path:
            inodes.add(fields[6])
def sockets(pid):
    found = {}
    for fd in os.listdir(f'/proc/{pid}/fd'):
        try:
            target = os.readlink(f'/proc/{pid}/fd/{fd}')
        except OSError:
            continue
        if target.startswith('socket:['):
            found[fd] = target[8:-1]
    return found
chain, pid = [], os.getpid()
while pid not in (0, 1, host):
    chain.append(pid)
    with open(f'/proc/{pid}/stat') as stat:
        text = stat.read()
    pid = int(text[text.rindex(')') + 2:].split()[1])
held = {str(p): sorted(i for i in sockets(p).values() if i in inodes) for p in chain}
host_held = {fd: inode for fd, inode in sockets(host).items() if inode in inodes}
cloexec = {}
for fd in host_held:
    with open(f'/proc/{host}/fdinfo/{fd}') as info:
        for line in info:
            if line.startswith('flags:'):
                cloexec[fd] = bool(int(line.split()[1], 8) & 0o2000000)
print(json.dumps({'inodes': sorted(inodes), 'reached_host': pid == host, 'chain': chain, 'held': held,
                  'host_held': host_held, 'cloexec': cloexec}))
'''

# A test-only `dsh` on PATH: execs the released harness with the keyless
# fixture model merged into the patch file hgs wrote. Every other argument, the
# environment and the process tree stay exactly as hgs made them.
SHIM = r'''
import json, os, sys
args = sys.argv[1:]
if '--patch' in args:
    at = args.index('--patch') + 1
    with open(args[at]) as f:
        original = json.load(f)
    merged = json.loads(os.environ['ZERUS_TEST_DSH_MODEL_PATCH']) + original
    path = os.path.join(os.path.dirname(args[at]), 'test-model.patch.json')
    with open(path, 'w') as f:
        json.dump(merged, f)
    args[at] = path
os.execv(os.environ['ZERUS_TEST_REAL_DSH'], [os.environ['ZERUS_TEST_REAL_DSH']] + args)
'''


def model_patch():
    return [{'id': 'llm-deepseek', 'disabled': True},
            {'id': 'agent-default-model', 'config': {'provider': 'hgs-test', 'model': 'test-a'}},
            {'id': 'schedule', 'disabled': True},
            {'insert': [{'id': 'hgs-test-model', 'name': str(REPO / 'tests/fixtures/dsh-model.mjs'), 'config': {}}]}]


class Hosted:
    """A private root, a launcher Claude session and helpers; subclasses start the host."""

    @classmethod
    def prepare(cls):
        fixture.admit_fixture(DSH, HGS)
        cls.temp = tempfile.TemporaryDirectory(prefix='hgs-dsh-id-', dir=os.environ.get('ZERUS_TEST_SHORT_TMPDIR', '/tmp'))
        cls.root = Path(cls.temp.name)
        for name in ('bin', 'user', 'native', 'state', 'config', 'tmux', 'ws', 'probes'):
            (cls.root / name).mkdir(mode=0o700)
        cls.home = cls.root / 'native'
        cls.state = cls.root / 'state'
        cls.host_dir = cls.state / 'dsh/hosts' / hashlib.sha256(str(cls.home).encode()).hexdigest()[:16]
        (cls.root / 'probe.py').write_text(PROBE)
        shim = cls.root / 'bin/dsh'
        shim.write_text('#!' + sys.executable + ' -I\n' + SHIM)
        shim.chmod(0o755)
        node = shutil.which('node')
        package = fixture.package_root(DSH)
        cls.env = {'PATH': os.pathsep.join([str(cls.root / 'bin'), str(Path(node).parent), '/usr/bin', '/bin']),
                   'LANG': 'C.UTF-8', 'HOME': str(cls.root / 'user'), 'DSH_HOME': str(cls.home),
                   'DSH_TELEMETRY_DISABLED': '1', 'HGS_DSH_TEST_PACKAGE': str(package),
                   'HGS_STATE_DIR': str(cls.state), 'HGS_CONFIG_DIR': str(cls.root / 'config'), 'HGS_SELF': 'test',
                   'HGS_PEERS': '', 'HGS_TAB': '0', 'TMUX_TMPDIR': str(cls.root / 'tmux'),
                   'ZERUS_TEST_REAL_DSH': str(Path(DSH).resolve()),
                   'ZERUS_TEST_DSH_MODEL_PATCH': json.dumps(model_patch())}
        # The launcher: a live Claude record whose agent, a stand-in process, is
        # never an ancestor of the host, as a real launcher never is.
        cls.stand_in = subprocess.Popen(['sleep', '3600'])
        start = subprocess.check_output(['ps', '-p', str(cls.stand_in.pid), '-o', 'lstart='],
                                        env={'LC_ALL': 'C', 'TZ': 'UTC', 'PATH': '/usr/bin:/bin'}, text=True).strip()
        cls.launcher = dict(version=1, name='claude/test/launcher', agent='claude', run_id='run-' + str(uuid.uuid4()),
                            session_id=str(uuid.uuid4()), conversation_id=str(uuid.uuid4()), pid=cls.stand_in.pid,
                            process_start=start, activity='idle', phase='idle', active_tools={}, subagents={},
                            last_event_at=time.time(), created=time.time())
        (cls.state / (hashlib.sha256(cls.launcher['name'].encode()).hexdigest() + '.json')).write_text(json.dumps(cls.launcher))
        cls.launcher_env = dict(cls.env, HGS_AGENT='claude', HGS_SESSION=cls.launcher['name'],
                                HGS_RUN_ID=cls.launcher['run_id'], HGS_SESSION_ID=cls.launcher['session_id'],
                                HGS_ACCOUNT_ID='native-claude', HGS_LAUNCH_ID='launch-of-the-launcher',
                                HGS_TERMINAL_NAME='launcher-terminal', TMUX='/tmp/launcher-tmux,1,0',
                                TMUX_PANE='%9', CLAUDECODE='1')
        cls.call('swarm', 'initialize', input=json.dumps({'projects': [
            {'id': 'project-1', 'name': 'orchard', 'color': '#123456', 'folders': [], 'sessions': []}]}))
        cls.call('account', 'permissions', 'native-dsh', '--mode', 'bypass')

    @classmethod
    def call(cls, *args, env=None, input=None, ok=True):
        result = subprocess.run([str(HGS), *args], input=input, env=env or cls.env, text=True,
                                capture_output=True, timeout=60, cwd=cls.root / 'ws')
        if ok and result.returncode != 0:
            raise AssertionError(f'hgs {args[0]} failed: {result.stderr}')
        return result

    @classmethod
    def finish(cls):
        try:
            cls.stand_in.kill()
            cls.stand_in.wait(timeout=5)
            cls.stop_host()
        finally:
            cls.temp.cleanup()

    @classmethod
    def stop_host(cls):
        """Stops only this fixture's own host: the one whose native home is private."""
        record = cls.host_dir / 'host.identity.json'
        pids = [json.loads(record.read_text())['pid']] if record.exists() else []
        pids += [cls.proc.pid] if getattr(cls, 'proc', None) else []
        for pid in pids:
            try:
                environ = Path(f'/proc/{pid}/environ').read_bytes() if sys.platform == 'linux' else b''
            except OSError:
                continue
            if sys.platform == 'linux' and f'DSH_HOME={cls.home}'.encode() not in environ.split(b'\0'):
                continue
            try:
                os.kill(pid, signal.SIGTERM)
            except ProcessLookupError:
                continue
            until = time.monotonic() + 15
            while time.monotonic() < until and Path(f'/proc/{pid}').exists():
                time.sleep(.1)

    @classmethod
    def rpc(cls, method, params=None):
        with socket.socket(socket.AF_UNIX) as client:
            client.settimeout(30)
            client.connect(str(cls.host_dir / 'host.sock'))
            client.sendall((json.dumps({'method': method, 'params': params or {}}) + '\n').encode())
            response = json.loads(client.makefile().readline())
        if not response['ok']:
            raise RuntimeError(response.get('error', str(response)))
        return response['value']

    @classmethod
    def binding(cls, tag):
        found = []
        for path in (cls.state / 'dsh').rglob('*.json'):
            try:
                value = json.loads(path.read_text())
            except (OSError, ValueError):
                continue
            if isinstance(value, dict) and value.get('backend') == 'dsh' and str(value.get('name', '')).endswith('/' + tag):
                found.append(value)
        assert len(found) == 1, (tag, found)
        return found[0]

    @classmethod
    def launch(cls, tag=None, new=True):
        tag = tag or 'id-' + uuid.uuid4().hex[:8]
        cls.call('dsh', str(cls.root / 'ws'), *(['--new'] if new else []), '-n', tag, '-d', env=cls.launcher_env)
        return cls.binding(tag)

    def shell(self, binding, mark='HGS_SELF_NATIVE_FIXTURE', runs=(), sleep=0, timeout=60, agent_id=None):
        """The environment of one hosted tool call of `binding` (or of its child
        `agent_id`), and its command results."""
        name = uuid.uuid4().hex[:12]
        out = self.root / 'probes' / (name + '.json')
        config = self.root / 'probes' / (name + '.cfg')
        config.write_text(json.dumps({'out': str(out), 'runs': list(runs), 'sleep': sleep}))
        command = f'{sys.executable} -I {self.root / "probe.py"} {config}'
        request = {'request_id': str(uuid.uuid4()), 'expected_run_id': binding['run_id'],
                   'expected_conversation_id': binding['conversation_id'], 'text': mark + ' ' + command}
        if agent_id:
            request.update(agent_id=agent_id, attachments=[])
        self.call('send', binding['name'], '--json', input=json.dumps(request))
        until = time.monotonic() + timeout
        while not out.exists() and time.monotonic() < until:
            time.sleep(.1)
        self.assertTrue(out.exists(), 'the hosted tool call did not run its probe')
        return json.loads(out.read_text())

    def assertOwnMarkers(self, env, binding):
        self.assertEqual({key: env.get(key) for key in ['HGS_AGENT', *SESSION_MARKERS]},
                         {'HGS_AGENT': 'dsh', 'HGS_SESSION': binding['name'], 'HGS_RUN_ID': binding['run_id'],
                          'HGS_SESSION_ID': binding['session_id'], 'HGS_ACCOUNT_ID': 'native-dsh'})
        self.assertNoLauncherValue(env)

    def assertNoMarkers(self, env):
        self.assertEqual(env.get('HGS_AGENT'), 'dsh')
        self.assertEqual([key for key in SESSION_MARKERS if key in env], [])
        self.assertNoLauncherValue(env)

    # Whether hgs started this host (its start drops the whole session context).
    STARTED_BY_HGS = True

    def assertNoLauncherValue(self, env):
        launcher = {self.launcher['name'], self.launcher['run_id'], self.launcher['session_id'], 'native-claude'}
        if self.STARTED_BY_HGS:
            launcher |= {'launch-of-the-launcher', '/tmp/launcher-tmux,1,0', '%9', 'launcher-terminal'}
            for key in ('TMUX', 'TMUX_PANE', 'CLAUDECODE', 'HGS_LAUNCH_ID', 'HGS_TERMINAL_NAME'):
                self.assertNotIn(key, env)
        self.assertEqual(sorted(key for key, value in env.items() if value in launcher), [])

    def task(self):
        added = json.loads(self.call('task', 'add', '--project', 'orchard', '--title', 'identity probe', '--json',
                                     input='Probe task for zerus-ppd26').stdout)
        return added['task']['ref']

    def person_actions(self, ref):
        """Commands only the person may run, or that record the person as their actor."""
        hgs = str(HGS)
        return [('start_plan', [hgs, 'task', 'start-plan', ref, '--json'], '{}'),
                ('add', [hgs, 'task', 'add', '--project', 'orchard', '--title', 'from a hosted call'], 'text'),
                ('comment', [hgs, 'task', 'comment', ref], 'a hosted comment')]

    def forwarded(self, ref):
        """A command another computer would forward for its agent (`--agent`)."""
        agent = str(uuid.uuid4()) + '/claude/elsewhere'
        return ('forwarded', [str(HGS), 'task', 'send', ref, '--agent', agent], None)

    def assertNotForwarded(self, result):
        self.assertNotEqual(result['code'], 0, result)
        self.assertIn('an agent acts as its own session', result['stderr'], result)

    def assertNeverThePerson(self, runs, ref):
        for name, result in runs.items():
            self.assertNotEqual(result['code'], 0, (name, result))
            self.assertIn('never acts as the person', result['stderr'], (name, result))
        history = json.loads(self.call('task', 'get', ref, '--history-limit', '20').stdout)['task']['log']
        self.assertEqual([entry['kind'] for entry in history], ['created'])


@unittest.skipUnless(sys.platform == 'linux' and (fixture.required() or DSH and HGS.exists()),
                     'official dsh, Linux /proc and built hgs required')
class HostStartedByHgs(Hosted, unittest.TestCase):
    """The host is started by hgs's own ensure_host, from the launcher's shell."""

    @classmethod
    def setUpClass(cls):
        cls.prepare()
        try:
            cls.first = cls.launch('first')
        except BaseException:
            cls.finish()
            raise

    @classmethod
    def tearDownClass(cls):
        cls.finish()

    def test_host_starts_without_the_launching_sessions_markers(self):
        pid = json.loads((self.host_dir / 'host.identity.json').read_text())['pid']
        environ = dict(entry.split('=', 1) for entry in Path(f'/proc/{pid}/environ').read_bytes().decode().split('\0')
                       if '=' in entry)
        self.assertEqual(environ.get('DSH_HOME'), str(self.home))
        self.assertEqual([key for key in SESSION_CONTEXT if key in environ], [])
        self.assertEqual(environ.get('HGS_STATE_DIR'), str(self.state))
        ping = self.rpc('ping')
        self.assertIs(ping['shellIdentity'], True)
        self.assertEqual(ping['version'], '0.2.1-rc.2')
        overlay = json.loads((self.host_dir / 'hgs.patch.json').read_text())
        self.assertEqual(overlay[0]['insert'][0]['config']['shellIdentities'], str(self.host_dir / 'shells'))

    def test_each_hosted_call_carries_only_its_own_session_markers(self):
        second = self.launch()
        self.assertOwnMarkers(self.shell(self.first)['env'], self.first)
        env = self.shell(second)['env']
        self.assertOwnMarkers(env, second)
        self.assertEqual(env['DSH_SESSION_ID'], second['conversation_id'])

    def test_new_runs_forks_children_and_jobs_carry_their_own_identity(self):
        source = self.launch()
        before = source['run_id']
        relaunched = self.launch(source['name'].rsplit('/', 1)[1], new=False)
        self.assertNotEqual(relaunched['run_id'], before)
        self.assertOwnMarkers(self.shell(relaunched)['env'], relaunched)
        self.call('pause', relaunched['name'])
        self.call('resume', relaunched['name'])
        resumed = self.binding(relaunched['name'].rsplit('/', 1)[1])
        self.assertNotIn(resumed['run_id'], (before, relaunched['run_id']))
        self.assertOwnMarkers(self.shell(resumed)['env'], resumed)
        tag = 'fork-' + uuid.uuid4().hex[:8]
        self.call('fork', resumed['name'], '-n', tag, '-d')
        fork = self.binding(tag)
        self.assertOwnMarkers(self.shell(fork)['env'], fork)
        # Forgetting a session removes the identity its calls carried.
        doomed = self.launch()
        identity = self.host_dir / 'shells' / (doomed['conversation_id'] + '.json')
        self.assertTrue(identity.exists())
        self.call('kill', doomed['name'])
        self.assertFalse(identity.exists())
        # Every path of a call: a child agent's call carries its parent's
        # markers; a background job and a promoted foreground command keep them.
        for mark, sleep in (('HGS_IDENTITY_CHILD_FIXTURE', 0), ('HGS_IDENTITY_BACKGROUND_FIXTURE', 0),
                            ('HGS_IDENTITY_PROMOTED_FIXTURE', 3)):
            with self.subTest(mark):
                session = self.launch()
                result = self.shell(session, mark, sleep=sleep, timeout=90)
                self.assertOwnMarkers(result['env'], session)
                if mark == 'HGS_IDENTITY_CHILD_FIXTURE':
                    self.assertNotEqual(result['env']['DSH_SESSION_ID'], session['conversation_id'])
                    # After the parent's next run starts, a later message to the
                    # child (outside any call of the parent) carries that new run:
                    # each call reads the current identity, none keeps an old one.
                    child = json.loads(self.call('inspect', session['name']).stdout)['subagents']
                    self.assertEqual(len(child), 1)
                    rerun = self.launch(session['name'].rsplit('/', 1)[1], new=False)
                    self.assertNotEqual(rerun['run_id'], session['run_id'])
                    followup = self.shell(rerun, 'HGS_IDENTITY_CHILD_CALL', agent_id=next(iter(child)), timeout=90)
                    self.assertOwnMarkers(followup['env'], rerun)
                    self.assertEqual(followup['env']['DSH_SESSION_ID'], result['env']['DSH_SESSION_ID'])

    def test_a_running_jobs_output_never_shows_its_live_token(self):
        # zerus-ppd26.5: a background job that prints its environment while it
        # runs keeps its token live; what the host's socket serves hides it.
        session = self.launch()
        seen = self.root / 'probes' / (uuid.uuid4().hex[:12] + '.token')
        script = ("import os, time; token = os.environ['HGS_DSH_CALL_TOKEN']; "
                  "print('HGS_DSH_CALL_TOKEN=' + token, flush=True); "
                  f"open({str(seen)!r}, 'w').write(token); time.sleep(60)")
        request = {'request_id': str(uuid.uuid4()), 'expected_run_id': session['run_id'],
                   'expected_conversation_id': session['conversation_id'],
                   'text': 'HGS_IDENTITY_BACKGROUND_FIXTURE ' + shlex.join([sys.executable, '-I', '-c', script])}
        self.call('send', session['name'], '--json', input=json.dumps(request))
        until = time.monotonic() + 60
        while not seen.exists() and time.monotonic() < until:
            time.sleep(.1)
        token = seen.read_text()
        self.assertRegex(token, '^[0-9a-f]{64}$')
        self.assertTrue(self.rpc('call-token', {'token': token})['known'], 'the running job keeps its token live')
        generation = self.rpc('ping')['generation']
        output = None
        while time.monotonic() < until:
            jobs = [job for job in self.rpc('inspect', {'sessionId': session['conversation_id']}).get('jobs') or []
                    if job.get('status') == 'running']
            if jobs:
                output = self.rpc('job-output', {'generation': generation, 'sessionId': session['conversation_id'],
                                                 'jobId': jobs[0]['id'], 'owner': jobs[0]['owner']})
                if 'HGS_DSH_CALL_TOKEN=' in output['output']:
                    break
            time.sleep(.2)
        self.assertIsNotNone(output)
        self.assertIn('HGS_DSH_CALL_TOKEN=[hidden call token]', output['output'])
        self.assertNotIn(token, json.dumps(output))
        # A reply and an error that echo a request carry no live token either
        # (zerus-ppd26.13, OWN1): the native error names the session it was given.
        echoed = self.rpc('session-action', {'sessionId': token, 'action': 'pause', 'generation': 'other'})
        self.assertEqual(echoed['sessionId'], '[hidden call token]')
        with self.assertRaises(RuntimeError) as refused:
            self.rpc('workspace', {'sessionId': token})
        self.assertIn('[hidden call token]', str(refused.exception))
        self.assertNotIn(token, str(refused.exception))
        self.rpc('job-stop', {'generation': generation, 'sessionId': session['conversation_id'],
                              'jobId': jobs[0]['id'], 'owner': jobs[0]['owner']})

    def test_no_tool_call_holds_a_descriptor_of_the_host_socket(self):
        # zerus-ppd26 Part B: peer credentials name the process that created the
        # host's listener, not the one that accepts; the proof relies on the host
        # never handing that listener to a child. A tool call's processes hold no
        # descriptor of host.sock, and the host's own are close-on-exec.
        session = self.launch()
        pid = json.loads((self.host_dir / 'host.identity.json').read_text())['pid']
        socket_path = str(self.host_dir / 'host.sock')
        for mark in ('HGS_SELF_NATIVE_FIXTURE', 'HGS_IDENTITY_BACKGROUND_FIXTURE'):
            with self.subTest(mark):
                runs = self.shell(session, mark, runs=[('fds', [sys.executable, '-I', '-c', FD_PROBE, socket_path, str(pid)],
                                                        None)], timeout=90)['runs']
                self.assertEqual(runs['fds']['code'], 0, runs['fds'])
                found = json.loads(runs['fds']['stdout'])
                self.assertTrue(found['inodes'] and found['reached_host'] and found['chain'], found)
                self.assertTrue(found['host_held'], 'the host must hold its listener: ' + json.dumps(found))
                self.assertEqual({pid: held for pid, held in found['held'].items() if held}, {}, found)
                self.assertTrue(found['cloexec'] and all(found['cloexec'].values()), found)

    def test_a_hosted_call_acts_as_its_own_session_and_no_other(self):
        # zerus-ppd26 Part B: the host's per-call token under the host's
        # ancestry proves the session; another session's markers, a forged or
        # an ended call's token prove nothing.
        own, other = self.launch(), self.launch()
        ref = self.task()
        hgs = str(HGS)
        refused_sends = []

        def unproven_send(label):
            request = {'request_id': str(uuid.uuid4()), 'expected_run_id': other['run_id'],
                       'expected_conversation_id': other['conversation_id'], 'text': 'UNPROVEN-' + label}
            refused_sends.append(request)
            return json.dumps(request)

        # A private peer stand-in proves that the attributed child crosses
        # the local found-host gate without clearing its hosted proof. It
        # never connects to an actual peer or runs a peer session.
        peer_log = self.root / 'proven-peer.json'
        peer_ssh = self.root / 'proven-peer-ssh'
        peer_ssh.write_text("""#!/usr/bin/env python3
import json, os, pathlib, sys
if 'hgs-remote-v2' in sys.argv[-1]:
    print('hgs-remote-v2|%OS%|%SystemRoot%|:OS|:SystemRoot'); sys.exit(0)
request = json.load(sys.stdin)
pathlib.Path(os.environ['PROVEN_PEER_LOG']).write_text(json.dumps(dict(argv=sys.argv[1:], request=request)))
print(json.dumps(dict(request_id=request['request_id'], run_id=request['expected_run_id'],
                     conversation_id=request['expected_conversation_id'], status='submitted')))
""")
        peer_ssh.chmod(0o700)
        orphan = 'orphan-' + uuid.uuid4().hex[:8]
        send = {'request_id': str(uuid.uuid4()), 'expected_run_id': other['run_id'],
                'expected_conversation_id': other['conversation_id'], 'text': 'from a hosted call'}
        remote_send = dict(send, request_id=str(uuid.uuid4()), text='from a hosted call to the private peer')
        result = self.shell(own, runs=[
            ('start', [hgs, 'task', 'start', ref], None),
            ('report', [hgs, 'task', 'report', ref], 'reported from the hosted call'),
            ('self', [hgs, 'self', '--json'], None),
            ('send', [hgs, 'send', other['name'], '--json'], json.dumps(send)),
            ('send_proven_peer', [hgs, '@peer', 'send', other['name'], '--json'], json.dumps(remote_send),
             {'HGS_PEERS': 'peer', 'HGS_SSH': str(peer_ssh), 'PROVEN_PEER_LOG': str(peer_log)}),
            ('impersonate', [hgs, 'task', 'comment', ref], 'as the other session',
             {'HGS_SESSION': other['name'], 'HGS_RUN_ID': other['run_id'], 'HGS_SESSION_ID': other['session_id']}),
            ('forged', [hgs, 'task', 'comment', ref], 'with a forged token', {'HGS_DSH_CALL_TOKEN': '0' * 64}),
            ('tokenless', [hgs, 'task', 'comment', ref], 'without a token', {'HGS_DSH_CALL_TOKEN': None}),
            # Without its proof a hosted call never sends or launches as nobody.
            ('send_tokenless', [hgs, 'send', other['name'], '--json'], unproven_send('no token'), {'HGS_DSH_CALL_TOKEN': None}),
            ('send_forged', [hgs, 'send', other['name'], '--json'], unproven_send('zero token'), {'HGS_DSH_CALL_TOKEN': '0' * 64}),
            ('launch_tokenless', [hgs, 'dsh', str(self.root / 'ws'), '--new', '-n', orphan, '-d'], None,
             {'HGS_DSH_CALL_TOKEN': None}),
            # Refused here, before any @host forwarding is built.
            ('remote_send', [hgs, '--dry-run', '@peer', 'send', other['name'], '--json'], unproven_send('remote'),
             {'HGS_DSH_CALL_TOKEN': None, 'HGS_PEERS': 'peer'}),
            ('remote_launch', [hgs, '--dry-run', '@peer', 'dsh', '/tmp', '--new', '-n', orphan, '-d'], None,
             {'HGS_DSH_CALL_TOKEN': None, 'HGS_PEERS': 'peer'}),
        ])
        runs = result['runs']
        token = result['env']['HGS_DSH_CALL_TOKEN']
        self.assertRegex(token, '^[0-9a-f]{64}$')
        for name in ('start', 'report', 'self', 'send', 'send_proven_peer'):
            self.assertEqual(runs[name]['code'], 0, (name, runs[name]))
        received = json.loads(peer_log.read_text())
        self.assertIn('__state send', received['argv'][-1])
        self.assertEqual(received['request']['request_id'], remote_send['request_id'])
        self.assertEqual(received['request']['text'].count('<zerus-message '), 1)
        self.assertIn(own['name'], received['request']['text'])
        identity = json.loads(runs['self']['stdout'])
        self.assertEqual((identity['identity_source'], identity['session_id']), ('hosted_call', own['session_id']))
        self.assertIn('One DeepSeek host process serves every session of its agent home', identity['proof_limit'])
        for name in ('impersonate', 'forged', 'tokenless'):
            self.assertNotEqual(runs[name]['code'], 0, (name, runs[name]))
            self.assertIn('never acts as the person', runs[name]['stderr'], (name, runs[name]))
        for name, missing in (('send_tokenless', 'carries no DeepSeek per-call token'),
                              ('send_forged', 'knows no live tool call with this token')):
            self.assertEqual(runs[name]['code'], 1, (name, runs[name]))
            self.assertIn('cannot prove its session, so its message is not sent', runs[name]['stderr'])
            self.assertIn(missing, runs[name]['stderr'])
        for name, sentence in (('launch_tokenless', 'does not launch a session without a parent'),
                               ('remote_send', 'so its message is not sent'),
                               ('remote_launch', 'does not launch a session without a parent')):
            self.assertEqual(runs[name]['code'], 1, (name, runs[name]))
            self.assertIn(sentence, runs[name]['stderr'], (name, runs[name]))
            self.assertNotIn('ssh', runs[name]['stdout'], (name, runs[name]))
        self.assertFalse(any(str(path).endswith(orphan) for path in (self.state / 'dsh').rglob('*')))
        log = json.loads(self.call('task', 'get', ref, '--history-limit', '20').stdout)['task']['log']
        acted = [entry for entry in log if entry['kind'] != 'created']
        self.assertTrue(acted)
        for entry in acted:
            self.assertEqual(entry['actor'].get('session'), own['name'], entry)
        self.assertNotIn('comment', [entry['kind'] for entry in log])
        import sqlite3
        rows = sqlite3.connect(f"file:{self.state / 'graph/sends.sqlite3'}?mode=ro", uri=True).execute(
            'SELECT payload FROM sends').fetchall()
        sources = [json.loads(row[0])['source'] for row in rows
                   if json.loads(row[0])['request_id'] == send['request_id']]
        self.assertEqual([source['name'] for source in sources], [own['name']])
        # The first call has ended, so its token now proves nothing.
        later = self.shell(own, runs=[('stale', [hgs, 'task', 'comment', ref], 'with an ended call token',
                                       {'HGS_DSH_CALL_TOKEN': token}),
                                      ('send_stale', [hgs, 'send', other['name'], '--json'], unproven_send('ended token'),
                                       {'HGS_DSH_CALL_TOKEN': token})])['runs']
        stale = later['stale']
        self.assertNotEqual(stale['code'], 0, stale)
        self.assertIn('never acts as the person', stale['stderr'])
        self.assertEqual(later['send_stale']['code'], 1, later['send_stale'])
        self.assertIn('so its message is not sent', later['send_stale']['stderr'])
        # Nothing of a refused send was recorded or delivered.
        refused = {request['request_id'] for request in refused_sends}
        rows = sqlite3.connect(f"file:{self.state / 'graph/sends.sqlite3'}?mode=ro", uri=True).execute(
            'SELECT payload FROM sends').fetchall()
        self.assertFalse(refused & {json.loads(row[0])['request_id'] for row in rows})
        delivered = json.dumps(json.loads(self.call('inspect', other['name']).stdout))
        self.assertIn('from a hosted call', delivered)
        self.assertNotIn('UNPROVEN-', delivered)

    def test_without_an_identity_a_hosted_call_is_never_the_person(self):
        session = self.launch()
        identity = self.host_dir / 'shells' / (session['conversation_id'] + '.json')
        self.assertTrue(identity.exists())
        ref = self.task()
        # With its identity the call is proven as its own session (Part B):
        # an agent, refused person-only start plans, acting as itself.
        result = self.shell(session, runs=self.person_actions(ref))
        self.assertOwnMarkers(result['env'], session)
        runs = result['runs']
        self.assertNotEqual(runs['start_plan']['code'], 0)
        self.assertIn("task start plans are for the person's explicit desktop Start", runs['start_plan']['stderr'])
        for name in ('add', 'comment'):
            self.assertEqual(runs[name]['code'], 0, (name, runs[name]))
        log = json.loads(self.call('task', 'get', ref, '--history-limit', '20').stdout)['task']['log']
        comments = [entry for entry in log if entry['kind'] == 'comment']
        self.assertEqual([entry['actor'].get('session') for entry in comments], [session['name']])
        ref = self.task()
        for damage in ('missing', 'invalid'):
            with self.subTest(damage):
                if damage == 'missing':
                    identity.unlink()
                else:
                    identity.write_text('{"version":1,"conversation":"' + session['conversation_id'] + '","session":""}')
                result = self.shell(session, runs=self.person_actions(ref) + [self.forwarded(ref)])
                self.assertNoMarkers(result['env'])
                self.assertNotForwarded(result['runs'].pop('forwarded'))
                self.assertNeverThePerson(result['runs'], ref)
        # The person outside the host still acts as the person.
        self.call('task', 'comment', ref, input='from the person')

    def test_a_hosted_call_cannot_switch_question_routing(self):
        # zerus-ppd26.1: with or without its identity file a hosted call is
        # refused before anything changes; the person outside the host is not.
        session = self.launch()
        identity = self.host_dir / 'shells' / (session['conversation_id'] + '.json')
        launcher = self.launcher
        request = json.dumps(dict(expected_run_id=launcher['run_id'], expected_conversation_id=launcher['conversation_id'],
                                  expected_revision=0, send_to_parent=False))
        routing = ('routing', [str(HGS), 'question-routing', launcher['name'], '--json'], request)
        for state in ('identity', 'missing'):
            with self.subTest(state):
                if state == 'missing':
                    identity.unlink()
                result = self.shell(session, runs=[routing])['runs']['routing']
                self.assertEqual(result['code'], 1, result)
                self.assertEqual(json.loads(result['stdout']), {'ok': False, 'error': 'agent_refused'})
        self.assertFalse((self.state / 'question-routing.json').exists())
        person = self.call('question-routing', launcher['name'], '--json', input=request)
        self.assertEqual(json.loads(person.stdout)['question_routing'], {'send_to_parent': False, 'revision': 1, 'person_off': False})


# The launching agent: a child subreaper that runs the launch itself, so the
# host it starts stays below it as it does while the launching hgs lives.
LAUNCHER = r"""
import ctypes, json, subprocess, sys
assert ctypes.CDLL(None, use_errno=True).prctl(36, 1, 0, 0, 0) == 0  # PR_SET_CHILD_SUBREAPER
for line in sys.stdin:
    request = json.loads(line)
    r = subprocess.run(request['argv'], env=request['env'], text=True, capture_output=True, timeout=120,
                       cwd=request['cwd'])
    print(json.dumps({'code': r.returncode, 'stdout': r.stdout, 'stderr': r.stderr}), flush=True)
"""


@unittest.skipUnless(sys.platform == 'linux' and (fixture.required() or DSH and HGS.exists()),
                     'official dsh, Linux /proc and built hgs required')
class LaunchingAgentAboveItsHost(Hosted, unittest.TestCase):
    """zerus-ppd26.15, the no-forgery window: the host is a child of the
    launching hgs, which is the launching agent's descendant, so the agent is
    an ancestor of every hosted tool call while that hgs lives. Here the
    launching agent keeps the host below it for the whole test."""

    @classmethod
    def setUpClass(cls):
        cls.prepare()
        try:
            cls.stand_in.kill()
            cls.stand_in.wait(timeout=5)
            cls.stand_in = subprocess.Popen([sys.executable, '-I', '-c', LAUNCHER], stdin=subprocess.PIPE,
                                            stdout=subprocess.PIPE, text=True)
            start = subprocess.check_output(['ps', '-p', str(cls.stand_in.pid), '-o', 'lstart='],
                                            env={'LC_ALL': 'C', 'TZ': 'UTC', 'PATH': '/usr/bin:/bin'},
                                            text=True).strip()
            cls.launcher.update(pid=cls.stand_in.pid, process_start=start)
            (cls.state / (hashlib.sha256(cls.launcher['name'].encode()).hexdigest() + '.json')).write_text(
                json.dumps(cls.launcher))
            request = {'argv': [str(HGS), 'dsh', str(cls.root / 'ws'), '--new', '-n', 'window', '-d'],
                       'env': cls.launcher_env, 'cwd': str(cls.root / 'ws')}
            cls.stand_in.stdin.write(json.dumps(request) + '\n')
            cls.stand_in.stdin.flush()
            launched = json.loads(cls.stand_in.stdout.readline())
            assert launched['code'] == 0, launched['stderr']
            cls.first = cls.binding('window')
        except BaseException:
            cls.finish()
            raise

    @classmethod
    def tearDownClass(cls):
        cls.finish()

    def ancestors(self, pid):
        chain = []
        while pid > 1 and len(chain) < 64:
            chain.append(pid)
            pid = int(Path(f'/proc/{pid}/stat').read_text().rsplit(')', 1)[1].split()[1])
        return chain

    def test_a_hosted_call_is_never_the_launching_agent_or_the_person(self):
        host = json.loads((self.host_dir / 'host.identity.json').read_text())['pid']
        self.assertIn(self.stand_in.pid, self.ancestors(host), 'the launching agent must stay above the host')
        ref = self.task()
        hgs = str(HGS)
        launcher = [f'HGS_SESSION={self.launcher["name"]}', f'HGS_RUN_ID={self.launcher["run_id"]}',
                    f'HGS_SESSION_ID={self.launcher["session_id"]}', 'HGS_AGENT=claude']
        bare = ['-u', 'HGS_AGENT', '-u', 'HGS_SESSION', '-u', 'HGS_RUN_ID', '-u', 'HGS_SESSION_ID',
                '-u', 'HGS_ACCOUNT_ID']
        runs = [('self', [hgs, 'self', '--json'], None),
                ('forged_self', ['/usr/bin/env', *launcher, hgs, 'self', '--json'], None),
                ('forged_comment', ['/usr/bin/env', *launcher, hgs, 'task', 'comment', ref], 'as the launcher'),
                ('person_comment', ['/usr/bin/env', *bare, hgs, 'task', 'comment', ref], 'as the person')]
        result = self.shell(self.first, runs=runs)
        self.assertOwnMarkers(result['env'], self.first)
        for name in ('self', 'forged_self'):
            run = result['runs'][name]
            if run['code'] == 0:
                answer = json.loads(run['stdout'])
                self.assertNotEqual(answer['name'], self.launcher['name'], (name, run))
                self.assertNotEqual(answer['identity_source'], 'process', (name, run))
        for name in ('forged_comment', 'person_comment'):
            run = result['runs'][name]
            self.assertNotEqual(run['code'], 0, (name, run))
            self.assertIn('This command runs under a DeepSeek agent host', run['stderr'], (name, run))
        history = json.loads(self.call('task', 'get', ref, '--history-limit', '20').stdout)['task']['log']
        self.assertEqual([entry['kind'] for entry in history], ['created'])


class DirectlyStarted(Hosted):
    """A host started outside hgs with an old launcher's markers in its environment."""
    LEGACY_BRIDGE = False
    STARTED_BY_HGS = False

    @classmethod
    def setUpClass(cls):
        cls.prepare()
        try:
            bridge = REPO / 'src/dsh/bridge.mjs'
            if cls.LEGACY_BRIDGE:
                # The same bridge without the per-call environment, like a host
                # an older hgs started: no wrapper, no capability in its ping.
                source = bridge.read_text()
                for old, new in (("        shellIdentity: callEnvironmentInstalled(), callTokens: callEnvironmentInstalled(),\n", ''),
                                 ('  function callEnvironmentInstalled() {\n',
                                  '  function callEnvironmentInstalled() { return false;\n')):
                    assert source.count(old) == 1, old
                    source = source.replace(old, new)
                bridge = cls.root / 'legacy-bridge.mjs'
                bridge.write_text(source)
            cls.host_dir.mkdir(parents=True, mode=0o700)
            patch = model_patch()
            patch[-1]['insert'].append({'id': 'hgs-native-dsh', 'name': str(bridge), 'config': {
                'socket': str(cls.host_dir / 'host.sock'), 'recoveryPolicy': str(cls.state / 'recovery/policy.json'),
                'shellIdentities': str(cls.host_dir / 'shells')}})
            overlay = cls.root / 'patch.json'
            overlay.write_text(json.dumps(patch))
            env = dict(cls.launcher_env)
            del env['ZERUS_TEST_DSH_MODEL_PATCH']
            cls.log = (cls.root / 'host.log').open('w')
            cls.proc = subprocess.Popen([DSH, '--profile', 'web', '--patch', str(overlay), '--no-open', '--host',
                                         '127.0.0.1', '--port', '0'], env=env, cwd=cls.root / 'ws',
                                        stdin=subprocess.DEVNULL, stdout=cls.log, stderr=cls.log, start_new_session=True)
            until = time.monotonic() + 40
            while not (cls.host_dir / 'host.sock').exists() and cls.proc.poll() is None and time.monotonic() < until:
                time.sleep(.1)
            if not (cls.host_dir / 'host.sock').exists():
                raise RuntimeError('Native test host did not start: ' + (cls.root / 'host.log').read_text()[-2500:])
            cls.session = cls.launch()
        except BaseException:
            cls.finish()
            raise

    @classmethod
    def tearDownClass(cls):
        cls.finish()
        cls.log.close()

    def host_markers(self):
        environ = Path(f'/proc/{self.proc.pid}/environ').read_bytes().decode().split('\0')
        return dict(entry.split('=', 1) for entry in environ if entry.startswith('HGS_') and '=' in entry)


@unittest.skipUnless(sys.platform == 'linux' and (fixture.required() or DSH and HGS.exists()),
                     'official dsh, Linux /proc and built hgs required')
class InheritedMarkersInTheHost(DirectlyStarted, unittest.TestCase):
    def test_a_call_never_passes_on_inherited_or_partial_markers(self):
        self.assertEqual(self.host_markers()['HGS_SESSION'], self.launcher['name'])
        identity = self.host_dir / 'shells' / (self.session['conversation_id'] + '.json')
        self.assertOwnMarkers(self.shell(self.session)['env'], self.session)
        ref = self.task()
        identity.unlink()
        result = self.shell(self.session, runs=self.person_actions(ref))
        self.assertNoMarkers(result['env'])
        self.assertNeverThePerson(result['runs'], ref)


@unittest.skipUnless(sys.platform == 'linux' and (fixture.required() or DSH and HGS.exists()),
                     'official dsh, Linux /proc and built hgs required')
class HostOfAnOlderHgs(DirectlyStarted, unittest.TestCase):
    LEGACY_BRIDGE = True

    def test_an_older_host_keeps_working_and_its_inherited_markers_stay_refused(self):
        ping = self.rpc('ping')
        self.assertNotIn('shellIdentity', ping)
        self.assertEqual(ping['version'], '0.2.1-rc.2')
        inspected = json.loads(self.call('inspect', self.session['name']).stdout)
        self.assertEqual(inspected['runtime_state'], 'live')
        ref = self.task()
        result = self.shell(self.session, runs=[('start', [str(HGS), 'task', 'start', ref], None)])
        # Never restarted or reloaded: its calls keep what it inherited.
        self.assertEqual({key: result['env'].get(key) for key in ('HGS_AGENT', 'HGS_SESSION')},
                         {'HGS_AGENT': 'claude', 'HGS_SESSION': self.launcher['name']})
        start = result['runs']['start']
        self.assertNotEqual(start['code'], 0)
        self.assertIn('names the agent session claude/test/launcher, but that run is not live', start['stderr'])
        # A host without per-call tokens proves no hosted call, even with that
        # session's own markers and a token-shaped value.
        claimed = {'HGS_AGENT': 'dsh', 'HGS_SESSION': self.session['name'], 'HGS_RUN_ID': self.session['run_id'],
                   'HGS_SESSION_ID': self.session['session_id'], 'HGS_DSH_CALL_TOKEN': 'a' * 64}
        runs = self.shell(self.session, runs=[('self', [str(HGS), 'self', '--json'], None, claimed),
                                              ('comment', [str(HGS), 'task', 'comment', ref], 'old host', claimed)])['runs']
        self.assertNotEqual(json.loads(runs['self']['stdout'] or '{}').get('identity_source'), 'hosted_call')
        self.assertNotEqual(runs['comment']['code'], 0)
        self.assertIn('never acts as the person', runs['comment']['stderr'])


if __name__ == '__main__':
    unittest.main()
