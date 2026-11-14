"""Peer commands keep their meaning in every login shell, including fish.

ssh hands its last argument to the peer's login shell as `<shell> -c <command>`.
POSIX snippets such as `if ...; then ...; fi` and `${SHELL:-/bin/sh}` are syntax
errors in fish, and fish also reads `\\\\` and `\\'` inside single quotes.
"""
import json
import os
import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
HGS = Path(os.environ.get('HGS_TEST_BIN', ROOT / 'target/debug/hgs'))

FAKE_SSH = r'''#!/usr/bin/env python3
import json, os, sys
fixture = os.environ['REMOTE_FIXTURE']
with open(os.path.join(fixture, 'ssh-args.json'), 'w') as file:
    json.dump(sys.argv[1:], file)
shell = os.environ['REMOTE_LOGIN_SHELL']
env = dict(os.environ, HOME=os.path.join(fixture, 'peer'), SHELL=os.path.join(fixture, 'peer-shell'))
os.execve(shell, [shell, '-c', sys.argv[-1]], env)
'''

# The peer's hgs lists every capability and records its exact arguments and input.
PEER_HGS = r'''#!/bin/sh
case "$1" in
--help)
  printf '%s\n' 'hgs [@host] wait' '  session-notifications-v1' 'hooks-health-v1' \
    'hgs [@host] analytics --json' 'hgs [@host] guide' 'hgs [@host] status'
  exit 0;;
--version) echo 'hgs 9.9.9'; exit 0;;
esac
python3 -c 'import json, os, sys; json.dump(sys.argv[1:], open(os.path.expanduser("~/argv.json"), "w"))' "$@"
cat > "$HOME/stdin"
printf '{"ok":true,"services":{}}\n'
'''

# What `exec "${SHELL:-/bin/sh}" -l` starts on the peer.
PEER_SHELL = r'''#!/bin/sh
pwd > "$HOME/shell-pwd"
printf '%s\n' "$@" > "$HOME/shell-args"
'''

PEER_TOOL = r'''#!/bin/sh
if [ "$1" = -V ]; then echo 'tmux 3.5'; fi
'''

TRICKY = "it's a\\b \\' \\\\ $HOME \"q\" ; echo bad `x` \\"


def login_shells():
    shells = {'sh': '/bin/sh'}
    for name in ('bash', 'zsh'):
        if path := shutil.which(name):
            shells[name] = path
    if fish := os.environ.get('HGS_TEST_FISH') or shutil.which('fish'):
        shells['fish'] = fish
    return shells


@unittest.skipIf(os.name == 'nt', 'Peers are Linux and macOS computers')
class RemoteLoginShells(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix='zerus-remote-shells-')
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.peer = self.root / 'peer'
        for path, text in ((self.root / 'fake-ssh', FAKE_SSH), (self.peer / '.local/bin/hgs', PEER_HGS),
                           (self.root / 'peer-shell', PEER_SHELL), (self.peer / '.cargo/bin/tmux', PEER_TOOL)):
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(text)
            path.chmod(0o755)
        home = self.root / 'home'
        (home / '.config/hgs').mkdir(parents=True)
        (home / '.config/hgs/config').write_text('HGS_SELF="test-local"\nHGS_PEERS="peer"\n')
        self.env = {key: value for key, value in os.environ.items()
                    if not key.startswith('HGS_') and key not in ('TMUX', 'TMUX_PANE', 'SSH_AUTH_SOCK')}
        self.env.update(HOME=str(home), HGS_CONFIG_DIR=str(home / '.config/hgs'), HGS_STATE_DIR=str(self.root / 'state'),
                        HGS_SSH=str(self.root / 'fake-ssh'), HGS_TAB='0', REMOTE_FIXTURE=str(self.root))

    def hgs(self, shell, *args, body=''):
        return subprocess.run([str(HGS), *args], input=body, text=True, capture_output=True, timeout=30,
                              env=dict(self.env, REMOTE_LOGIN_SHELL=shell))

    def received(self):
        argv = json.loads((self.peer / 'argv.json').read_text())
        stdin = (self.peer / 'stdin').read_text()
        for name in ('argv.json', 'stdin'):
            (self.peer / name).unlink()
        return argv, stdin

    def test_fish_is_present_where_ci_requires_it(self):
        if os.environ.get('GITHUB_ACTIONS') == 'true' and sys.platform.startswith('linux'):
            self.assertIn('fish', login_shells())
        elif 'fish' not in login_shells():
            self.skipTest('fish is not installed; set HGS_TEST_FISH to run the fish cases')

    def test_capability_probes_run_under_sh_and_keep_json_input(self):
        body = json.dumps(dict(request_id='r-1', text=TRICKY))
        cases = [(('@peer', 'wait', 'ack', '--json'), ['wait', 'ack', '--json']),
                 (('@peer', 'notifications', '--json'), ['notifications', '--json']),
                 (('@peer', 'hooks', 'claude/example/task', '--json'), ['hooks', 'claude/example/task', '--json']),
                 (('@peer', 'analytics', '--json'), ['analytics', '--json', '--read-only']),
                 (('@peer', 'guide'), ['guide'])]
        for name, shell in login_shells().items():
            for args, expected in cases:
                with self.subTest(shell=name, command=args[1]):
                    result = self.hgs(shell, *args, body=body)
                    self.assertEqual(result.returncode, 0, result.stderr)
                    self.assertEqual(self.received(), (expected, body))

    def test_path_info_reaches_only_a_peer_whose_help_names_it(self):
        # An older hgs would take the unknown verb as a session launch (zerus-5wmh.13.18).
        executable = self.peer / '.local/bin/hgs'
        path = '/srv/' + TRICKY
        for name, shell in login_shells().items():
            with self.subTest(shell=name):
                executable.write_text(PEER_HGS)
                old = self.hgs(shell, '@peer', 'path-info', '--path', path)
                self.assertNotEqual(old.returncode, 0)
                self.assertIn('update the selected computer', old.stderr.lower())
                self.assertFalse((self.peer / 'argv.json').exists())
                executable.write_text(PEER_HGS.replace("'hooks-health-v1'", "'hooks-health-v1' 'hgs [@host] path-info --path <path>'"))
                result = self.hgs(shell, '@peer', 'path-info', '--path', path)
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertEqual(self.received(), (['path-info', '--path', path], ''))
                # ~ arrives unexpanded: the peer's hgs expands ~/ and refuses ~user.
                for tilde in ("~/plan's notes.md", '~other/plan.md'):
                    self.assertEqual(self.hgs(shell, '@peer', 'path-info', '--path', tilde).returncode, 0)
                    self.assertEqual(self.received(), (['path-info', '--path', tilde], ''))
                self.assertNotIn('-t', json.loads((self.root / 'ssh-args.json').read_text()))

    def test_a_guarded_path_info_needs_both_markers_and_keeps_its_tilde(self):
        # --expect-machine wraps the actual command; guarded-create-v1 alone
        # never authorizes path-info (zerus-3skhm), and the wrapped path's
        # ~ arrives as written too (zerus-ie12o).
        machine = '22222222-2222-4222-8222-222222222222'
        executable = self.peer / '.local/bin/hgs'
        for name, shell in login_shells().items():
            with self.subTest(shell=name):
                executable.write_text(PEER_HGS.replace("'hooks-health-v1'", "'hooks-health-v1' 'guarded-create-v1'"))
                old = self.hgs(shell, '@peer', '--expect-machine', machine, 'path-info', '--path', '~/plan.md')
                self.assertNotEqual(old.returncode, 0)
                self.assertIn('update the selected computer', old.stderr.lower())
                self.assertFalse((self.peer / 'argv.json').exists())
                executable.write_text(PEER_HGS.replace("'hooks-health-v1'", "'hooks-health-v1' 'hgs [@host] path-info --path <path>'"))
                unguarded = self.hgs(shell, '@peer', '--expect-machine', machine, 'path-info', '--path', '~/plan.md')
                self.assertNotEqual(unguarded.returncode, 0, "a peer without guarded-create-v1 cannot check the machine")
                self.assertFalse((self.peer / 'argv.json').exists())
                executable.write_text(PEER_HGS.replace("'hooks-health-v1'", "'hooks-health-v1' 'guarded-create-v1' 'hgs [@host] path-info --path <path>'"))
                for path in ('~/plan.md', "~/plan's notes.md", '~other/plan.md', '/srv/' + TRICKY):
                    result = self.hgs(shell, '@peer', '--expect-machine', machine, 'path-info', '--path', path)
                    self.assertEqual(result.returncode, 0, result.stderr)
                    argv = self.received()[0]
                    self.assertEqual((argv[:2], argv[-3:]), (['--expect-machine', machine], ['path-info', '--path', path]))
                self.assertNotIn('-t', json.loads((self.root / 'ssh-args.json').read_text()))

    def test_plain_commands_deliver_quotes_and_backslashes_exactly(self):
        for name, shell in login_shells().items():
            with self.subTest(shell=name):
                result = self.hgs(shell, '@peer', 'search', TRICKY)
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertEqual(self.received()[0], ['search', TRICKY])
                version = self.hgs(shell, '@peer', 'version')
                self.assertEqual((version.returncode, version.stdout), (0, 'hgs 9.9.9\n'), version.stderr)

    def test_guarded_creation_requires_exact_peer_capability_and_keeps_identity(self):
        identity='11111111-1111-4111-8111-111111111111'
        for name,shell in login_shells().items():
            with self.subTest(shell=name):
                executable=self.peer/'.local/bin/hgs'
                executable.write_text(PEER_HGS)
                old=self.hgs(shell,'@peer','--expect-machine',identity,'dirs',TRICKY)
                self.assertNotEqual(old.returncode,0);self.assertIn('update the selected computer',old.stderr)
                self.assertFalse((self.peer/'argv.json').exists())
                executable.write_text(PEER_HGS.replace("'hooks-health-v1'","'hooks-health-v1' 'guarded-create-v1'"))
                result=self.hgs(shell,'@peer','--expect-machine',identity,'dirs',TRICKY)
                self.assertEqual(result.returncode,0,result.stderr)
                self.assertEqual(self.received(),(['--expect-machine',identity,'--client','test-local','dirs',TRICKY],''))
                options=json.loads((self.root/'ssh-args.json').read_text())
                self.assertIn('BatchMode=yes',options);self.assertNotIn('-t',options)

    def test_a_session_id_reaches_only_a_peer_that_advertises_the_selector(self):
        # zerus-ucck P1: an older peer would read --session-id as a session name,
        # so it never sees the flag; a current peer receives it unchanged.
        identity = '22222222-2222-4222-8222-222222222222'
        for name, shell in login_shells().items():
            with self.subTest(shell=name):
                executable = self.peer / '.local/bin/hgs'
                executable.write_text(PEER_HGS)
                old = self.hgs(shell, '@peer', 'inspect', '--session-id', identity)
                self.assertNotEqual(old.returncode, 0)
                self.assertIn('update the selected computer to select sessions by ID', old.stderr)
                self.assertFalse((self.peer / 'argv.json').exists())
                for invalid in (['--session-id'], ['--session-id', 'NOT-A-UUID'],
                                ['--session-id', identity, '--session-id', identity]):
                    refused = self.hgs(shell, '@peer', 'inspect', *invalid)
                    self.assertEqual(refused.returncode, 2, refused.stderr)
                    self.assertFalse((self.peer / 'argv.json').exists())
                executable.write_text(PEER_HGS.replace("'hooks-health-v1'", "'hooks-health-v1' '  session-id-selector-v1'"))
                result = self.hgs(shell, '@peer', 'inspect', '--session-id', identity)
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertEqual(self.received()[0], ['inspect', '--session-id', identity])

    def test_every_selector_call_negotiates_the_selector_with_its_peer(self):
        # #1355 review: every session command, a guarded call that wraps one and
        # the native UI's own peer call require the selector; a peer that only
        # lists what the call needed before never receives the flag.
        import re
        identity = '22222222-2222-4222-8222-222222222222'
        machine = '11111111-1111-4111-8111-111111111111'
        table = re.search(r'const SESSION_COMMANDS: &\[&str\] = &\[(.*?)\];', (ROOT / 'src/cli.rs').read_text(), re.S)
        commands = re.findall(r'"([a-z_-]+)"', table.group(1))
        self.assertIn('terminate', commands)
        self.assertIn('send-now', commands)
        # A remote lifecycle receipt read is refused unless a guarded call wraps it.
        calls = [['@peer', command, '--session-id', identity] for command in commands if command != 'session-action-status']
        calls.append(['@peer', '--expect-machine', machine, 'inspect', '--session-id', identity])
        calls.append(['@peer', '--expect-machine', machine, 'session-action-status', '--session-id', identity, '--json'])
        calls.append(['@peer', 'native-ui', '--session-id', identity, '--json'])
        # The DeepSeek attach alias opens the same native UI (#1355 review R3).
        calls.append(['@peer', 'a', 'dsh/p/example', '--session-id', identity])
        # zerus-ucck P1c: a reference to the peer's session, with @peer and
        # routed by the reference's own host.
        reference = ('zerus://session?machine=33333333-3333-4333-8333-333333333333&host=peer'
                     '&name=codex%2Fp%2Fa&conversation=c&run=r')
        calls.append(['@peer', 'send', reference, '--json'])
        calls.append(['routed', 'pause', reference])
        executable = self.peer / '.local/bin/hgs'
        older = PEER_HGS.replace("'hooks-health-v1'", "'hooks-health-v1' 'Desktop creation ABI: guarded-create-v1' '  lifecycle-receipt-status-v1'")
        executable.write_text(older)
        def run(call):
            return self.hgs('/bin/sh', *(call[1:] if call[0] == 'routed' else call))
        for call in calls:
            with self.subTest(call=call[1:3], peer='older'):
                refused = run(call)
                self.assertNotEqual(refused.returncode, 0, refused.stdout)
                self.assertIn('update the selected computer to select sessions by ID', refused.stderr)
                self.assertFalse((self.peer / 'argv.json').exists())
        # A current peer lists every capability these calls need, including
        # keep-warm's own (#1686, zerus-on03r).
        executable.write_text(older.replace("'  lifecycle-receipt-status-v1'",
                                            "'  lifecycle-receipt-status-v1' '  session-id-selector-v1' '  session-reference-selector-v1'"
                                            " '  session-keep-warm-v1' '  session-question-routing-v1'"))
        for call in calls:
            with self.subTest(call=call[1:3], peer='current'):
                run(call)
                argv = self.received()[0]
                if call[1] == 'native-ui':
                    self.assertEqual(argv, ['__dsh-ui-url', '--session-id', identity])
                elif call[1:3] == ['a', 'dsh/p/example']:
                    self.assertEqual(argv, ['__dsh-ui-url', 'dsh/p/example', '--session-id', identity])
                elif call[1] == '--expect-machine':
                    self.assertEqual(argv[:2], call[1:3], argv)
                    self.assertEqual(argv[-len(call[3:]):], call[3:], argv)
                else:
                    if reference in call:
                        # The one verified hop is guarded by the reference's machine.
                        self.assertEqual(argv[:2], ['--expect-machine', '33333333-3333-4333-8333-333333333333'], argv)
                    self.assertEqual(argv[-len(call[1:]):], call[1:], argv)

    def test_an_unproven_hosted_call_forwards_nothing_as_nobody(self):
        # zerus-ppd26 Part B: a resident DeepSeek host's tool call (HGS_AGENT=dsh)
        # without its hosted proof is never the person, and a peer cannot see
        # who calls. Every command a session reference or --expect-machine
        # forwards with a machine, every send and every launch without --parent
        # is refused before ssh runs; the person, and a launch with --parent,
        # go on. The commands are read from the source tables.
        import re
        source = (ROOT / 'src/cli.rs').read_text()
        def table(name):
            found = re.search(r'const ' + name + r': &\[&str\] = &\[(.*?)\];', source, re.S)
            return re.findall(r'"([a-z_-]+)"', found.group(1))
        session_commands = table('SESSION_COMMANDS')
        self.assertIn('send', session_commands)
        # The launch verbs are pinned here, not only read from the table they
        # check (zerus-ppd26.10): dropping one from the table fails this test.
        launches = ['claude', 'codex', 'kimi', 'dsh', 'opencode', 'fork']
        self.assertEqual(table('LAUNCH_COMMANDS'), launches)
        machine = '33333333-3333-4333-8333-333333333333'
        reference = (f'zerus://session?machine={machine}&host=peer&name=codex%2Fp%2Fa&conversation=c&run=r')
        (self.peer / '.local/bin/hgs').write_text(PEER_HGS.replace(
            "'hooks-health-v1'", "'hooks-health-v1' 'Desktop creation ABI: guarded-create-v1' '  lifecycle-receipt-status-v1'"
            " '  session-id-selector-v1' '  session-reference-selector-v1' '  session-keep-warm-v1'"
            " '  claimed-parent-v1' '  launch-parent-v1'"))
        hosted = dict(HGS_AGENT='dsh')
        person = {}
        parent = '{"session_id":"s","run_id":"r","conversation_id":"c"}'
        calls = []
        for command in session_commands:
            extra = ['--json'] if command in ('send', 'session-action-status') else []
            calls.append(('reference', [command, reference, *extra]))
            calls.append(('guarded', ['@peer', '--expect-machine', machine, command, 'codex/p/a', *extra]))
        for command in launches:
            target = 'codex/p/a' if command == 'fork' else '/tmp'
            calls.append(('launch', ['@peer', command, target]))
        # A word that only looks like --parent names no parent: the value of
        # another option, or an agent flag after -- (zerus-ppd26.10).
        for shape in (['claude', '-n', '--parent'], ['claude', '--model', '--parent'], ['codex', '--account', '--parent'],
                      ['fork', 'codex/p/a', '-n', '--parent'], ['claude', '/tmp', '--', '--parent', parent],
                      ['dsh', '/tmp', '--parent'], ['kimi', '/tmp', '--parent', 'not a claim'],
                      # The value of -n or --account, followed by a real claim
                      # that the peer reads as the project folder.
                      ['claude', '-n', '--parent', parent], ['codex', '--account', '--parent', parent],
                      ['fork', 'codex/p/a', '-n', '--parent', parent],
                      # Neither --parent=CLAIM (no launch parser reads it) nor a
                      # claim after the value of --parent-link (U5, U6).
                      ['claude', '/tmp', '--parent=' + parent], ['claude', '--parent-link', '--parent', parent]):
            calls.append(('shape', ['@peer', *shape]))
        sent = self.root / 'ssh-args.json'

        def run(call, extra):
            sent.unlink(missing_ok=True)
            for name in ('argv.json', 'stdin'):
                (self.peer / name).unlink(missing_ok=True)
            env = {k: v for k, v in self.env.items()}
            env.update(extra, REMOTE_LOGIN_SHELL='/bin/sh')
            return subprocess.run([str(HGS), *call], input='{}', text=True, capture_output=True, timeout=30, env=env)

        refusals = ('cannot prove its session', "Open another computer's native UI by name")
        for route, call in calls:
            with self.subTest(route=route, call=call[:2] if route != 'guarded' else call[3:5], caller='hosted'):
                refused = run(call, hosted)
                self.assertEqual(refused.returncode, 1, refused.stdout + refused.stderr)
                if 'question-routing' in call:
                    # This operation's machine-readable refusal is on stdout;
                    # it still fails before any remote hop or native action.
                    self.assertEqual(json.loads(refused.stdout), dict(ok=False, error='agent_refused'))
                else:
                    self.assertTrue(any(text in refused.stderr for text in refusals), refused.stderr)
                self.assertFalse(sent.exists(), 'ssh must not run')
                self.assertFalse((self.peer / 'argv.json').exists())
        forwarded = 0
        for route, call in calls:
            with self.subTest(route=route, call=call[:2] if route != 'guarded' else call[3:5], caller='person'):
                result = run(call, person)
                self.assertNotIn('cannot prove its session', result.stderr)
                if (self.peer / 'argv.json').exists():
                    forwarded += 1
        # Most of them really reach the peer for the person; none for the call.
        self.assertGreater(forwarded, len(calls) // 2)
        for command in launches:
            target = 'codex/p/a' if command == 'fork' else '/tmp'
            for route, call in (('launch', ['@peer', command, target, '--parent', parent]),
                                ('reference', [command, reference, '--parent', parent]) if command == 'fork' else (None, None)):
                if route is None:
                    continue
                with self.subTest(route=route, call=command, caller='hosted with --parent'):
                    result = run(call, hosted)
                    self.assertNotIn('cannot prove its session', result.stderr)
                    self.assertTrue(sent.exists(), result.stdout + result.stderr)

    def test_a_reference_takes_exactly_one_verified_hop(self):
        # #1376 review: a reference reaches its computer through one SSH hop the
        # peer verifies; a guarded call never forwards a reference again.
        machine = '33333333-3333-4333-8333-333333333333'
        reference = (f'zerus://session?machine={machine}&host=peer&name=codex%2Fp%2Fa&conversation=c&run=r')
        executable = self.peer / '.local/bin/hgs'
        executable.write_text(PEER_HGS.replace("'hooks-health-v1'", "'hooks-health-v1' '  session-reference-selector-v1'"))
        dry = self.hgs('/bin/sh', '--dry-run', 'kill', reference)
        self.assertEqual(dry.returncode, 0, dry.stderr)
        self.assertIn(f'--expect-machine {machine} kill', dry.stdout)
        self.assertNotIn(' -t ', dry.stdout, 'the hop keeps the SSH options of the command')
        local = json.loads((self.root / 'home/.config/hgs/swarm/catalog.json').read_text())['node_id']
        self.assertNotEqual(local, machine)
        # A guarded call that verified this computer refuses another one's reference.
        for call in (['--expect-machine', local, 'send', reference, '--json'],
                     ['--expect-machine', local, '--client', 'peer', 'pause', reference],
                     ['@test-local', '--expect-machine', local, 'kill', reference],
                     ['--expect-machine', local, 'inspect', reference]):
            with self.subTest(call=call):
                refused = self.hgs('/bin/sh', *call)
                self.assertEqual(refused.returncode, 1, refused.stdout + refused.stderr)
                self.assertIn('a guarded call acts only on this computer', refused.stderr)
                self.assertFalse((self.root / 'ssh-args.json').exists())
        # The receiving side of a hop that reached another computer refuses it.
        hop = self.hgs('/bin/sh', '--expect-machine', machine, '--client', 'peer', 'pause', reference)
        self.assertEqual(hop.returncode, 1, hop.stdout + hop.stderr)
        self.assertIn('the selected SSH alias does not match', hop.stderr)
        self.assertFalse((self.root / 'ssh-args.json').exists())
        # The hop itself: one SSH call to the alias, guarded by the reference's machine.
        result = self.hgs('/bin/sh', '@peer', 'send', reference, '--json', body='{}')
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(self.received(), (['--expect-machine', machine, 'send', reference, '--json'], '{}'))

    def test_peer_status_reads_the_status_command(self):
        for name, shell in login_shells().items():
            with self.subTest(shell=name):
                result = self.hgs(shell, '@peer', 'status', '--json')
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertEqual(self.received()[0], ['status', '--local', '--json'])

    def test_machine_check_reads_the_setup_report(self):
        for name, shell in login_shells().items():
            with self.subTest(shell=name):
                result = self.hgs(shell, 'machine', 'check', 'peer')
                self.assertEqual(result.returncode, 0, result.stderr)
                report = json.loads(result.stdout)
                self.assertTrue(report['ok'], report)
                self.assertEqual((report['hgs'], report['tmux']), ('hgs 9.9.9', 'tmux 3.5'))

    def test_machine_shell_opens_the_folder_in_the_login_shell(self):
        folder = self.root / "a folder/it's \\ $(x)"
        folder.mkdir(parents=True)
        for name, shell in login_shells().items():
            with self.subTest(shell=name):
                result = self.hgs(shell, 'machine', 'ssh', 'peer', '--directory', str(folder))
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertEqual((self.peer / 'shell-pwd').read_text(), f'{folder}\n')
                self.assertEqual((self.peer / 'shell-args').read_text(), '-l\n')
                (self.peer / 'shell-pwd').unlink()


    def test_stripped_readable_agent_ancestors_stop_person_routes_before_ssh(self):
        # The child loses its own markers, but its private launching Python
        # process remains a readable marked ancestor. No real peer is used.
        import uuid
        strip = r'''import os,subprocess,sys
env=dict(os.environ)
for key in ('HGS_SESSION','HGS_RUN_ID','HGS_SESSION_ID'):env.pop(key,None)
result=subprocess.run(sys.argv[1:],input=sys.stdin.read(),env=env,text=True,capture_output=True,timeout=30)
sys.stdout.write(result.stdout);sys.stderr.write(result.stderr);sys.exit(result.returncode)
'''
        marked = dict(self.env, REMOTE_LOGIN_SHELL=login_shells()['sh'],
                      HGS_SESSION='codex/private-ancestor', HGS_RUN_ID=str(uuid.uuid4()),
                      HGS_SESSION_ID=str(uuid.uuid4()))
        machine = '55555555-5555-4555-8555-555555555555'
        for command in ('task', 'team', 'question-routing'):
            forms = (['@peer', command, 'x', '--json'],
                     ['--dry-run', '@peer', command, 'x', '--json'],
                     ['@peer', '--expect-machine', machine, command, 'x', '--json'],
                     ['@peer', '--expect-machine', machine, '--client', 'other', command, 'x', '--json'],
                     ['@peer', '__state', command, 'x', '--json'],
                     ['@peer', '--expect-machine', machine, '__state', command, 'x', '--json'])
            for form in forms:
                with self.subTest(form=form):
                    (self.root / 'ssh-args.json').unlink(missing_ok=True)
                    result = subprocess.run([sys.executable, '-c', strip, str(HGS), *form],
                                            env=marked, input='', text=True, capture_output=True, timeout=40)
                    self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
                    self.assertIn("outside the agent's process tree", result.stderr)
                    self.assertFalse((self.root / 'ssh-args.json').exists(), 'no capability probe or SSH')
                    self.assertFalse(result.stdout.startswith('ssh '), 'dry run refuses too')
        for form in (['@peer', 'inspect', 'x'], ['@peer', 'interrupt', 'x'],
                     ['@peer', 'wait', 'ack', '--json'], ['@peer', 'answer', 'x', '--json']):
            with self.subTest(agent_safe=form):
                (self.root / 'ssh-args.json').unlink(missing_ok=True)
                result = subprocess.run([sys.executable, '-c', strip, str(HGS), *form],
                                        env=marked, input='', text=True, capture_output=True, timeout=40)
                self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
                self.assertTrue((self.root / 'ssh-args.json').exists())

    def test_a_person_only_command_never_reaches_another_computer_from_an_agent(self):
        # zerus-ppd26.9: another computer's hgs sees no marker of the agent that
        # calls it and takes it for the person. So an agent's call of a command
        # in PERSON_ONLY_FORWARDED stops before ssh on every route, --dry-run
        # included, while the person's goes on. Commands an agent may use on
        # another computer still forward for it.
        import hashlib
        import re
        import uuid
        source = (ROOT / 'src/cli.rs').read_text()

        def table(name):
            found = re.search(r'const ' + name + r': &\[&str\] = &\[(.*?)\];', source, re.S)
            return re.findall(r'"([a-z_-]+)"', found.group(1))

        person_only = table('PERSON_ONLY_FORWARDED')
        self.assertEqual(sorted(person_only), ['question-routing', 'task', 'team'])
        state = self.root / 'state'
        state.mkdir(exist_ok=True)

        def started(pid):
            return subprocess.check_output(['ps', '-p', str(pid), '-o', 'lstart='],
                                           env=dict(self.env, LC_ALL='C', TZ='UTC'), text=True).strip()

        def record(name, run, pid, conversation):
            value = dict(version=1, name=name, agent='claude', run_id=run, conversation_id=conversation,
                         session_id=str(uuid.uuid4()), pid=pid, process_start=started(pid), activity='idle',
                         phase='idle', active_tools={}, subagents={},
                         claude_registry_home=str(self.root / 'claude'))
            (state / (hashlib.sha256(name.encode()).hexdigest() + '.json')).write_text(json.dumps(value))

        # A proven agent: this test process is its agent.
        record('claude/example/agent', 'run-agent', os.getpid(), str(uuid.uuid4()))
        # A Claude background job, with no HGS markers: its session's window
        # holds the job this process runs under.
        window = subprocess.Popen(['sleep', '60'])
        self.addCleanup(window.wait)
        self.addCleanup(window.kill)
        conversation, job = str(uuid.uuid4()), str(uuid.uuid4())
        record('claude/example/window', 'run-window', window.pid, conversation)
        registry = self.root / 'claude/sessions'
        registry.mkdir(parents=True)
        (registry / f'{window.pid}.json').write_text(json.dumps(dict(pid=window.pid, kind='interactive', parkedJobId=job)))
        (registry / f'{os.getpid()}.json').write_text(
            json.dumps(dict(pid=os.getpid(), kind='bg', jobId=job, sessionId=conversation)))
        agents = {
            'proven': dict(HGS_SESSION='claude/example/agent', HGS_RUN_ID='run-agent'),
            'unproven': dict(HGS_SESSION='claude/example/gone', HGS_RUN_ID='run-gone'),
            'hosted': dict(HGS_AGENT='dsh'),
            'background': dict(CLAUDE_PID=str(os.getpid()), CLAUDE_CODE_SESSION_ID=conversation,
                               CLAUDE_CONFIG_DIR=str(self.root / 'claude')),
        }
        shell = login_shells()['sh']

        def run(args, extra):
            (self.root / 'ssh-args.json').unlink(missing_ok=True)
            result = subprocess.run([str(HGS), *args], input='', text=True, capture_output=True, timeout=30,
                                    env=dict(self.env, REMOTE_LOGIN_SHELL=shell, **extra))
            return result, (self.root / 'ssh-args.json').exists() or result.stdout.startswith('ssh ')

        def refused(command, name, result):
            # This guard's refusal, or one that answers first once the branches
            # carrying them are merged (zerus-ppd26.12): batch 7's JSON refusal
            # of question-routing, and ppd26 Part B's of an unproven DeepSeek
            # call on a route that names a machine. Each stops before ssh.
            here = f'hgs {command} on another computer would act as the person' in result.stderr
            try:
                routed = (command == 'question-routing'
                          and json.loads(result.stdout) == {'ok': False, 'error': 'agent_refused'})
            except ValueError:
                routed = False
            hosted = name == 'hosted' and 'cannot prove its session' in result.stderr
            return result.returncode == 1 and (here or routed or hosted)

        machine = '55555555-5555-4555-8555-555555555555'
        # A session reference's verified hop is a route too: a person-only
        # session command (batch 7's question-routing) is checked there.
        reference = f'zerus://session?machine={machine}&host=peer&name=codex%2Fp%2Fa&conversation=c&run=r'
        for command in sorted(set(person_only) & set(table('SESSION_COMMANDS'))):
            with self.subTest(reference=command):
                result, reached = run(['--dry-run', command, reference, '--json'], {})
                self.assertIn(f'--expect-machine {machine} {command}', result.stdout, result.stderr)
                for name, extra in agents.items():
                    result, reached = run(['--dry-run', command, reference, '--json'], extra)
                    self.assertFalse(reached, name)
                    self.assertTrue(refused(command, name, result), (name, result.stdout, result.stderr))
        for command in person_only:
            forms = (['@peer', command, 'x', '--json'],
                     ['--dry-run', '@peer', command, 'x', '--json'],
                     ['@peer', '--expect-machine', machine, command, 'x', '--json'],
                     ['@peer', '--expect-machine', machine, '--client', 'other', command, 'x', '--json'],
                     ['@peer', '__state', command, 'x', '--json'],
                     ['@peer', '--expect-machine', machine, '__state', command, 'x', '--json'])
            for form in forms:
                with self.subTest(form=' '.join(form)):
                    result, reached = run(form, {})
                    self.assertTrue(reached, result.stdout + result.stderr)
                    for name, extra in agents.items():
                        result, reached = run(form, extra)
                        self.assertFalse(reached, name)
                        self.assertTrue(refused(command, name, result), (name, result.stdout, result.stderr))
        for form in (['@peer', 'answer', 'x', '--json'], ['@peer', 'interrupt', 'x'], ['@peer', 'wait', 'ack', '--json'],
                     ['@peer', 'inspect', 'x']):
            for name, extra in agents.items():
                with self.subTest(form=' '.join(form), agent=name):
                    result, reached = run(form, extra)
                    self.assertTrue(reached, (name, result.stderr))

    def test_agent_sandbox_answers_stop_before_ssh_and_other_answers_keep_exact_input(self):
        # All SSH, including capability probes, goes to this private PATH fake.
        bindir = self.root/'bin'; bindir.mkdir()
        ssh = bindir/'ssh'; ssh.write_text(FAKE_SSH); ssh.chmod(0o700)
        self.env.pop('HGS_SSH', None)
        self.env['PATH'] = str(bindir)+os.pathsep+self.env['PATH']
        executable = self.peer/'.local/bin/hgs'
        executable.write_text(PEER_HGS.replace("'hooks-health-v1'", "'hooks-health-v1' 'guarded-create-v1'"))
        shell = login_shells()['sh']
        machine = '55555555-5555-4555-8555-555555555555'
        forms = (['@peer', 'answer', 'codex/p/a', '--json'],
                 ['@peer', '--expect-machine', machine, 'answer', 'codex/p/a', '--json'],
                 ['@peer', '--expect-machine', machine, '--client', 'fixture', 'answer', 'codex/p/a', '--json'],
                 ['@peer', '__state', 'answer', 'codex/p/a', '--json'],
                 ['@peer', '--expect-machine', machine, '__state', 'answer', 'codex/p/a', '--json'],
                 ['@peer', '--expect-machine', machine, '--client', 'fixture', '__state', 'answer', 'codex/p/a', '--json'])
        callers = ({}, dict(HGS_SESSION='codex/fixture/agent', HGS_RUN_ID='fixture-run'), dict(HGS_AGENT='dsh'))
        for form in forms:
            for caller in callers:
                for question in ('codex-sandbox:'+'a'*64, 'ordinary-question', 'other-source:'+'b'*64):
                    with self.subTest(form=form, caller=caller, question=question):
                        (self.root/'ssh-args.json').unlink(missing_ok=True)
                        body = ' \n'+json.dumps(dict(question_id=question, request_id='fixture-request',
                            expected_run_id='run', expected_conversation_id='',
                            answers=[dict(question_id='sandbox', selected_option_ids=['sandbox_2'], text='Пример \\')]),
                            ensure_ascii=False, indent=2)+'\n '
                        result = subprocess.run([str(HGS), *form], input=body, text=True,
                            capture_output=True, timeout=30, env=dict(self.env, REMOTE_LOGIN_SHELL=shell, **caller))
                        protected = bool(caller) and question.startswith('codex-sandbox:')
                        if protected:
                            self.assertEqual(result.returncode, 1, result.stdout+result.stderr)
                            self.assertIn('person', result.stderr)
                            self.assertFalse((self.root/'ssh-args.json').exists())
                        elif caller.get('HGS_AGENT') == 'dsh' and '--expect-machine' in form:
                            # This pre-existing hosted-proof refusal also
                            # stays intact for every other guarded answer.
                            self.assertEqual(result.returncode, 1, result.stdout+result.stderr)
                            self.assertIn('cannot prove its session', result.stderr)
                            self.assertFalse((self.root/'ssh-args.json').exists())
                        else:
                            self.assertEqual(result.returncode, 0, result.stdout+result.stderr)
                            _, received = self.received()
                            self.assertEqual(received, body)

    def test_oversized_agent_sandbox_answer_never_reaches_ssh(self):
        body = json.dumps(dict(question_id='codex-sandbox:'+'a'*64, padding='x'*(128*1024)))
        result = subprocess.run([str(HGS), '@peer', 'answer', 'codex/p/a', '--json'], input=body,
            text=True, capture_output=True, timeout=30, env=dict(self.env,
                REMOTE_LOGIN_SHELL=login_shells()['sh'], HGS_SESSION='codex/fixture/agent', HGS_RUN_ID='run'))
        self.assertEqual(result.returncode, 1, result.stdout+result.stderr)
        self.assertIn('answer payload exceeds128 KiB', result.stderr)
        self.assertFalse((self.root/'ssh-args.json').exists())


if __name__ == '__main__':
    unittest.main()
