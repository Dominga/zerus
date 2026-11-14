"""Isolated real-agent fixture for Agent Teams (milestone 1, package 15).

One private root per test class holds HOME, every XDG folder, the runtime folder,
the hgs state and configuration, the agents' account homes, TMPDIR and an existing
0700 TMUX_TMPDIR. A tmux wrapper first on PATH refuses any call that names no
server inside the root; ssh, scp, systemd-run and hgs are refusing shims, so the
worktree build runs only by its absolute path (HGS_TEST_BIN) and nothing reaches
another computer, the user manager or an installed hgs. The real agent is the
pinned image the runner selected (HGS_<AGENT>_TEST_BIN). Its model is a keyless
loopback fixture that answers text or a scripted shell step, so every place where
the Teams service touches a session runs against the real agent without any
account or model quota.

A private `hgs desktop serve --background` with the Teams preview on launches the
lead and the member; the person answers their startup cards (folder trust in this
round; the standing confirmations come after zerus-63rru.48 and .49). Every wait
names what it waits for, has an explicit bound and logs its measured duration as
`ZERUS_TEAMS_WAIT=` lines and in the evidence folder. A socket census samples the
fixture's own processes and fails on any peer that is neither loopback nor an
allow-listed host of the agent under test.
"""
import json
import os
from pathlib import Path
import pwd
import re
import shlex
import shutil
import signal
import socket
import stat
import subprocess
import sys
import tempfile
import threading
import time
import unittest
import uuid

import model_api_fixture as api

HGS = os.environ.get('HGS_TEST_BIN', '')
# Set by a runner that keeps custody of private roots (scripts/ci/kimi-native.py
# --teams): called with the case name and the root as soon as the root exists.
ROOT_CREATED = None
REAL_TMUX = shutil.which('tmux') or '/usr/bin/tmux'
SYSTEM_PATH = '/usr/local/bin:/usr/bin:/bin' if sys.platform != 'darwin' else '/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin'
FIXTURE_KEY = 'fixture-key'
# The children's account name: the runner's own never reaches an agent that talks to an
# outside host. The preflight fails if the real name is the value of any variable.
FIXTURE_USER = 'teams-fixture'
DONE = 'Fixture complete.'
REPLY = 'Fixture reply.'
# Hosts each agent contacts whatever its switches say (measured 2026-11-12 with Claude
# Code 2.1.322 and Codex 0.167.2; docs/ci-and-aur.md lists them with the switches set).
# Any other non-loopback peer fails the class.
ALLOWED_HOSTS = {
    'claude': ('downloads.claude.ai',),
    'codex': ('raw.githubusercontent.com', 'ab.chatgpt.com'),
    'kimi': (),
}
# Bounds are about three times the duration measured on a loaded development host
# (the upstream canary has no Teams step yet); the logged durations tighten them.
BOUND = dict(service=60, bound=420, listed=90, card=180, ready=180, parent=200, handout=240, turn=180,
             doorbell=240, cancel=240, stop=180, exit=60, teardown=60)
# The private root's name. It is short so that a member worktree in a hosted runner's
# /tmp (`/tmp/hgs-tn-XXXXXXXX/work/zerus-team-<uuid>`, 73 columns) fits one row of
# Codex's Folder access panel; a local root inside the worktree's target/ wraps.
ROOT_PREFIX = 'hgs-tn-'
# Codex's Folder access panel shows the folder in rows of this many columns in the
# member's 80-column pane (measured with Codex 0.167.2); a longer folder continues on
# the rows below.
CODEX_PANEL_COLUMNS = 76


def hosted_runner(env=None):
    """A disposable GitHub-hosted runner, where the private root sits in a short /tmp."""
    env = os.environ if env is None else env
    return env.get('GITHUB_ACTIONS') == 'true' and env.get('RUNNER_ENVIRONMENT') == 'github-hosted'


def codex_panel_variant(pane, folder):
    """Which Folder access panel `pane` shows for `folder`: 'one-row' when the folder
    fills the row under the panel's title, 'wrapped' when it continues on the rows
    below, None when the pane does not show that folder there."""
    lines = [line.strip() for line in pane.splitlines()]
    if 'Folder access' not in lines:
        return None
    joined = ''
    for count, line in enumerate(lines[lines.index('Folder access') + 1:], 1):
        joined += line
        if not line or not folder.startswith(joined):
            return None
        if joined == folder:
            return 'one-row' if count == 1 else 'wrapped'
    return None

TMUX_WRAPPER = r'''#!/bin/bash
# Teams fixture tmux wrapper: logs every call; passes it to the real tmux only when it
# names a server inside the private root (-S inside it, $TMUX inside it, or no socket
# with the existing private TMUX_TMPDIR, judged by the socket path tmux will use).
# Links are resolved before judging: an existing path, a link included, must resolve
# inside the root, so a dangling link refuses; a path that does not exist yet is judged
# by its nearest existing ancestor, resolved. Anything else is refused.
ROOT=@ROOT@
REAL=@REAL@
args=("$@"); n=${#args[@]}; sock=""; label=""; idx=0; version_only=0
while (( idx < n )); do
  a=${args[idx]}
  case "$a" in
    --) break ;;
    -S) sock=${args[idx+1]}; idx=$((idx+2)) ;;
    -S*) sock=${a#-S}; idx=$((idx+1)) ;;
    -L) label=${args[idx+1]}; idx=$((idx+2)) ;;
    -L*) label=${a#-L}; idx=$((idx+1)) ;;
    -f|-c|-T) idx=$((idx+2)) ;;
    -V) version_only=1; idx=$((idx+1)) ;;
    -*) idx=$((idx+1)) ;;
    *) break ;;
  esac
done
resolve() {
  local p=$1 rest="" d
  while [[ ! -e "$p" && ! -L "$p" ]]; do rest="/$(basename -- "$p")$rest"; p=$(dirname -- "$p"); done
  d=$(realpath -e -- "$p" 2>/dev/null) || return 1
  printf '%s%s\n' "$d" "$rest"
}
inside() { local p; p=$(resolve "$1") || return 1; [[ "$p" == "$ROOT"/* ]]; }
verdict=refused
if [[ -n "$sock" ]]; then inside "$sock" && verdict=private
elif [[ -n "${TMUX:-}" ]]; then inside "${TMUX%%,*}" && verdict=private
elif [[ -n "${TMUX_TMPDIR:-}" && -d "$TMUX_TMPDIR" ]] && inside "$TMUX_TMPDIR" \
     && inside "$TMUX_TMPDIR/tmux-$(id -u)/${label:-default}"; then verdict=private
elif (( version_only == 1 && n == 1 )); then verdict=version
fi
printf '%s\t%s\t%s\n' "$(date +%s.%N)" "$verdict" "${1:-}" >> @LOGS@/tmux-calls.log
if [[ $verdict == refused ]]; then
  echo "Teams fixture: refused a tmux call that names no server inside the private root" >&2
  exit 1
fi
exec "$REAL" "$@"
'''

REFUSE = r'''#!/bin/bash
# Teams fixture: no other computer, no new unit, no hgs from PATH.
printf '%s\t%s\n' "$(date +%s.%N)" "$(basename -- "$0")" >> @LOGS@/refused-calls.log
echo "Teams fixture: $(basename -- "$0") is refused in this private root" >&2
exit 97
'''

# The hosted Kimi workflow only: `hgs account inspect` signs a Kimi account in only
# through api.kimi.com. With an expiring token hgs runs `node` with its bundled module
# src/accounts/kimi-read.mjs, the producer of this value; this stand-in answers that
# one call. Its value is built by kimi_inspection() from the producer's own starting
# object, changing named keys only to what the producer writes for a signed-in profile
# with no usage windows; a guard test compares both with kimi-read.mjs. Account choice
# for Kimi is therefore not evidence here; everything after it is.
KIMI_PRODUCER_BASE = {'identity': {}, 'windows': [], 'source': 'Kimi Code', 'status': 'unavailable'}


def kimi_inspection():
    value = json.loads(json.dumps(KIMI_PRODUCER_BASE))
    value['identity'] = {'name': 'Fixture', 'email': '', 'plan': '', 'auth_method': 'Kimi Code'}
    return value


KIMI_NODE = r'''#!/bin/bash
printf '%s\tnode %s\n' "$(date +%s.%N)" "${1:-}" >> @LOGS@/kimi-inspection-stand-in.log
if [[ "${1:-}" == "--input-type=module" && "${2:-}" == "-e" && -n "${KIMI_CODE_HOME:-}" ]]; then
  printf '%s\n' @VALUE@
  exit 0
fi
exit 97
'''

KIMI_FD = r'''#!/bin/bash
printf '%s\tfd %s\n' "$(date +%s.%N)" "$*" >> @LOGS@/fd-stand-in.log
if [[ $# -eq 1 && "${1:-}" == "--version" ]]; then
  printf 'fd fixture\n'
  exit 0
fi
exit 97
'''


def now():
    return time.monotonic()


class Evidence:
    """Bounded evidence: wait timings, requests, census rows. Written as files."""

    def __init__(self, folder):
        self.folder = Path(folder)
        self.folder.mkdir(parents=True, exist_ok=True)
        self.lock = threading.Lock()

    def append(self, name, row):
        with self.lock, (self.folder / name).open('a', encoding='utf-8') as f:
            f.write(json.dumps(row, ensure_ascii=False, default=str) + '\n')

    def write(self, name, value):
        (self.folder / name).write_text(value if isinstance(value, str) else json.dumps(value, indent=2, default=str),
                                        encoding='utf-8')


PINNED = r"`hgs task {verb} (?:'([^'`]+)'|([^\s'`]+)) --attempt ([0-9a-f]{{8}}-[0-9a-f]{{4}}-[0-9a-f]{{4}}-[0-9a-f]{{4}}-[0-9a-f]{{12}})"


def pinned(prompt, verb):
    """The (ref, attempt) of the pinned `hgs task <verb> REF --attempt UUID` in a real
    delivered prompt, the ref quoted when it holds a space; None when it is not pinned."""
    match = re.search(PINNED.format(verb=verb), prompt)
    return (match.group(1) or match.group(2), match.group(3)) if match else None


def acknowledgement(prompt):
    """Use the managed pin when present, otherwise the explicit legacy Start pin.
    The report origin alone never acknowledges a delivery.
    """
    begin = pinned(prompt, 'begin-attempt')
    if begin:
        return ('begin-attempt', *begin)
    match = re.search(r"\[Zerus task start\]\nAcknowledge this task with `hgs task start (?:'([^'`]+)'|([^\s'`]+))`", prompt)
    return ('start', match.group(1) or match.group(2), '') if match else None


def wait_for_held_task(hgs, reference, bound):
    """Wait for the service's own delivery outcome before the fast stand-in acts.
    The model request can arrive before the CLI records its submitted receipt.
    Read the original task's UUID only; never select a report origin from a lookup.
    Called in the member's own shell, preserving its caller proof.
    """
    task = json.loads(subprocess.check_output([hgs, 'task', 'get', reference, '--json'], text=True, timeout=30))
    task = task.get('task') or task
    uuid = task['uuid']
    end = now() + bound
    while True:
        current = json.loads(subprocess.check_output([hgs, 'team', 'current', '--json'], text=True, timeout=30))
        nodes = TeamFixture.listed(current, 'nodes', 'team current')
        if any((node.get('task') or {}).get('uuid') == uuid for node in nodes):
            return current
        if now() >= end:
            raise AssertionError('the service did not record the original task delivery before the member acknowledgement')
        time.sleep(0.1)


class Model(api.Handler):
    """The shared loopback fixture plus the Teams steps: a delivered task prompt and a
    typed `ZT-RUN <step>` each answer with one shell call that runs a fixture script."""

    def do_POST(self):
        route = api.urlsplit(self.path).path.rstrip('/')
        try:
            body = json.loads(self.rfile.read(int(self.headers.get('Content-Length') or 0)) or b'{}')
        except ValueError:
            body = {}
        body = body if isinstance(body, dict) else {}
        number = next(self.server.numbers)
        answer = self.server.plan(body)
        self.server.log_request(route, number, body, answer)
        try:
            if route.endswith('/count_tokens'):
                return self.reply(200, {'input_tokens': 100})
            if route.endswith('/messages'):
                message = api.anthropic_message(body, answer, number)
                return self.stream(api.anthropic_events(message)) if body.get('stream') else self.reply(200, message)
            if route.endswith('/responses'):
                return self.stream(api.responses_events(body, answer, number))
            if route.endswith('/chat/completions'):
                return self.chat(answer, number)
            if '/kimi-platform' in route:
                return self.reply(404, {'error': 'no platform in the fixture'})
            self.reply(404, {'error': {'type': 'not_found_error', 'message': 'no such route'}})
        except (BrokenPipeError, ConnectionResetError, ConnectionAbortedError):
            pass

    def do_GET(self):
        route = api.urlsplit(self.path).path.rstrip('/')
        if '/kimi-platform' in route:
            return self.reply(404, {'error': 'no platform in the fixture'})
        if route.endswith('/models'):
            return self.reply(200, {'object': 'list', 'data': [{'id': 'fixture', 'object': 'model'}], 'has_more': False})
        self.reply(404, {'error': {'type': 'not_found_error', 'message': 'no such route'}})

    def chat(self, answer, number):
        self.send_response(200)
        self.send_header('Content-Type', 'text/event-stream')
        self.end_headers()

        def chunk(delta, finish=None):
            data = dict(id=f'chat_fixture_{number}', object='chat.completion.chunk', model='fixture',
                        choices=[dict(index=0, delta=delta, finish_reason=finish)])
            self.wfile.write(b'data: ' + json.dumps(data).encode() + b'\n\n')
        if answer[0] == 'tool':
            chunk(dict(role='assistant', content=None, tool_calls=[dict(index=0, id=f'call_fixture_{number}', type='function',
                  function=dict(name=answer[1], arguments=json.dumps(answer[2])))]))
            chunk({}, 'tool_calls')
        else:
            chunk(dict(role='assistant', content=answer[1]))
            chunk({}, 'stop')
        self.wfile.write(b'data: [DONE]\n\n')
        self.wfile.flush()


def chat_tools(body):
    return [t.get('function', {}).get('name') for t in body.get('tools') or [] if isinstance(t, dict)]


def chat_results(body):
    found = []
    for item in reversed(body.get('messages') or []):
        if isinstance(item, dict) and item.get('role') == 'tool':
            found.append(item.get('content'))
        elif isinstance(item, dict) and item.get('role') == 'user':
            break
    return found[::-1]


def user_texts(body):
    """Every typed user text of the request, oldest first."""
    return [text for item in api.items(body) for text in api.typed_parts(item)]


class ModelServer(api.Server):
    def __init__(self, root, scripts, evidence):
        super().__init__(root, '')
        self.RequestHandlerClass = Model
        self.scripts = Path(scripts)
        self.evidence = evidence
        self.requests = []

    def shell(self, offered, command):
        if 'Bash' in offered:
            return ('tool', 'Bash', {'command': command, 'description': 'Zerus Teams fixture step'})
        if 'exec_command' in offered or 'exec' in offered:
            # Codex's native shell subtool, called directly (tests/test_busy_input_native.py).
            return ('tool', 'exec_command', {'cmd': command, 'yield_time_ms': 30000, 'max_output_tokens': 4000})
        return ('text', 'Fixture: no shell tool offered: ' + ','.join(offered))

    def plan(self, body):
        offered = api.tool_names(body) or chat_tools(body)
        prompt = (api.typed(body) or '').strip()
        if api.results(body) or chat_results(body):
            return ('text', DONE)
        step = re.fullmatch(r'ZT-RUN ([a-z0-9-]{1,40})', prompt)
        if step and (self.scripts / (step.group(1) + '.sh')).is_file():
            return self.shell(offered, '/bin/bash ' + shlex.quote(str(self.scripts / (step.group(1) + '.sh'))))
        if prompt.startswith('[Zerus task ') and not prompt.split('\n', 1)[0].rstrip().endswith('cancelled]'):
            begin = acknowledgement(prompt)
            report = pinned(prompt, 'report-attempt')
            if report:
                # Follow the original prompt's acknowledgement, including legacy Start.
                script = self.scripts / 'delivered.sh'
                return self.shell(offered, ' '.join(shlex.quote(x) for x in (
                    '/bin/bash', str(script), *(begin[1:] if begin else ('', '')), *report,
                    begin[0] if begin else '')))
            return ('text', 'Fixture: the delivered prompt has no pinned commands')
        # Anything else is the shared fixture's own plan (a reply, or its questions).
        return api.plan(body, '')

    def log_request(self, route, number, body, answer):
        row = dict(n=number, at=time.time(), route=route, prompt=api.typed(body), users=user_texts(body),
                   tools=(api.tool_names(body) or chat_tools(body))[:40],
                   results=[str(r)[:4000] for r in (api.results(body) or chat_results(body))], answer=list(answer[:2]))
        with self.lock:
            self.requests.append(row)
        self.evidence.append('model-requests.jsonl', row)


class Census(threading.Thread):
    """Samples the fixture's own processes (descendants of this test process and of the
    private tmux server) and their TCP/UDP peers once a second. A peer that is neither
    loopback nor an address an allow-listed host resolves to fails the class. Sampled:
    a connection opened and closed between two samples is not seen."""

    def __init__(self, evidence, hosts):
        super().__init__(daemon=True)
        self.evidence, self.hosts = evidence, tuple(hosts)
        self.roots = {os.getpid()}
        self.stop_event = threading.Event()
        self.allowed = {}  # ip -> host
        self.seen, self.violations, self.counts, self.samples = set(), [], {}, 0
        self.resolved_at = 0

    def resolve(self):
        if now() - self.resolved_at < 20:
            return
        self.resolved_at = now()
        for host in self.hosts:
            try:
                for info in socket.getaddrinfo(host, 443, proto=socket.IPPROTO_TCP):
                    self.allowed[info[4][0]] = host
            except OSError:
                pass

    @staticmethod
    def table():
        parents = {}
        for name in os.listdir('/proc'):
            if name.isdigit():
                try:
                    text = open(f'/proc/{name}/stat').read()
                    parents[int(name)] = int(text[text.rindex(')') + 2:].split()[1])
                except (OSError, ValueError):
                    pass
        return parents

    def owned(self):
        parents = self.table()
        children = {}
        for pid, parent in parents.items():
            children.setdefault(parent, []).append(pid)
        out, todo = set(), [p for p in self.roots if p in parents]
        while todo:
            pid = todo.pop()
            if pid not in out:
                out.add(pid)
                todo += children.get(pid, [])
        return out

    @staticmethod
    def peers(pid):
        table = {}
        for proto in ('tcp', 'tcp6', 'udp', 'udp6'):
            try:
                lines = open(f'/proc/{pid}/net/{proto}').read().splitlines()[1:]
            except OSError:
                continue
            for line in lines:
                f = line.split()
                host, port = f[2].split(':')
                if int(port, 16) == 0:
                    continue
                raw = bytes.fromhex(host)
                if len(raw) == 16:
                    raw = b''.join(raw[i:i + 4][::-1] for i in range(0, 16, 4))
                    ip = socket.inet_ntop(socket.AF_INET6, raw)
                    if ip.startswith('::ffff:'):
                        ip = ip[7:]
                else:
                    ip = socket.inet_ntop(socket.AF_INET, raw[::-1])
                table[int(f[9])] = (proto, ip, int(port, 16))
        return table

    def sample(self):
        self.samples += 1
        self.resolve()
        table = None
        for pid in self.owned():
            try:
                inodes = {int(os.readlink(f'/proc/{pid}/fd/{fd}')[8:-1]) for fd in os.listdir(f'/proc/{pid}/fd')
                          if os.readlink(f'/proc/{pid}/fd/{fd}').startswith('socket:[')}
            except OSError:
                continue
            if not inodes:
                continue
            if table is None:
                table = self.peers(pid)
            for inode in inodes & table.keys():
                if (pid, inode) in self.seen:
                    continue
                self.seen.add((pid, inode))
                proto, ip, port = table[inode]
                loopback = ip.startswith('127.') or ip == '::1'
                host = 'loopback' if loopback else self.allowed.get(ip)
                try:
                    comm = open(f'/proc/{pid}/comm').read().strip()
                except OSError:
                    comm = '?'
                row = dict(at=time.time(), pid=pid, comm=comm, proto=proto, peer=f'{ip}:{port}', host=host)
                self.evidence.append('census.jsonl', row)
                self.counts[host or 'not allowed'] = self.counts.get(host or 'not allowed', 0) + 1
                if host is None:
                    self.violations.append(row)

    def run(self):
        while not self.stop_event.wait(1.0):
            try:
                self.sample()
            except Exception as error:  # A census failure is evidence too, never silent.
                self.violations.append(dict(error=str(error)[:300]))

    def finish(self):
        self.stop_event.set()
        self.join(10)
        self.sample()
        summary = dict(samples=self.samples, allow_list=list(self.hosts), counts=self.counts,
                       violations=self.violations[:50])
        self.evidence.write('census-summary.json', summary)
        return summary


class FolderWatch(threading.Thread):
    """Whether one folder exists, sampled every 0.2 s until finished: when it was first
    seen gone and when it came back."""

    def __init__(self, path):
        super().__init__(daemon=True)
        self.path, self.stopping = Path(path), threading.Event()
        self.result = dict(path=str(path), started=time.time(), samples=0, gone_at=None, back_at=None)

    def run(self):
        while not self.stopping.is_set():
            exists = self.path.is_dir()
            self.result['samples'] += 1
            if not exists and self.result['gone_at'] is None:
                self.result['gone_at'] = time.time()
            if exists and self.result['gone_at'] is not None and self.result['back_at'] is None:
                self.result['back_at'] = time.time()
            self.stopping.wait(0.2)

    def finish(self):
        self.stopping.set()
        self.join(2)
        self.result['finished'] = time.time()
        return self.result


class TeamFixture:
    """One private root, one private service, one team (lead and member m1) of one agent."""

    DELEGATIONS_OFF = {'accept_results': False, 'answer_members': False, 'confirm_trust': False,
                       'merge_accepted': False, 'resume_exited': False}

    def __init__(self, agent, native, case, delegations=None, service_answers=(), after_refusal=None, count=1, pinned=None,
                 pretrust=()):
        self.agent, self.native, self.case = agent, Path(native).resolve(), case
        # Card sources the person answers only once the service has refused them with a
        # card naming the member and containing the given text.
        self.after_refusal = dict(after_refusal or {})
        # The team's delegations, and the startup card sources the person leaves to the
        # standing confirmation (it must answer them, or the bring-up fails).
        self.delegations = dict(self.DELEGATIONS_OFF, **(delegations or {}))
        self.service_answers = tuple(service_answers)
        # The role's member count, and the account its members are pinned to (none: the
        # service picks the freest).
        self.count, self.pinned = count, pinned
        # Accounts on which the person trusts the main checkout before the team starts.
        self.pretrust = tuple(pretrust)
        self.cards_seen = {}
        self.panes = {}
        base = os.environ.get('TMPDIR') or tempfile.gettempdir()
        self.root = Path(tempfile.mkdtemp(prefix=ROOT_PREFIX, dir=base)).resolve()
        if ROOT_CREATED is not None:
            ROOT_CREATED(case, self.root)
        folder = os.environ.get('ZERUS_TEAMS_EVIDENCE') or str(self.root / 'evidence')
        self.evidence = Evidence(Path(folder) / case)
        self.waits = []
        self.processes = []
        self.service = None
        self.team = None
        self.sessions = {}
        self.census = None
        self.model = None

    # ---- environment -----------------------------------------------------------
    def build(self):
        r = self.root
        for sub in ('bin', 'log', 'home/.local/bin', 'home/.config', 'home/.local/share', 'home/.cache',
                    'home/.local/state', 'run', 'tmux', 'tmp', 'state', 'cfg', 'scripts', 'out', 'model', 'work'):
            (r / sub).mkdir(parents=True, exist_ok=True)
        os.chmod(r / 'run', 0o700)
        # The person's answers start as an empty log, so a reader can tell "none" from "lost".
        (self.evidence.folder / 'person-answers.jsonl').write_text('')
        os.chmod(r / 'tmux', 0o700)
        for name, text in (('tmux', TMUX_WRAPPER), ('ssh', REFUSE), ('scp', REFUSE), ('systemd-run', REFUSE), ('hgs', REFUSE)):
            path = r / 'bin' / name
            path.write_text(text.replace('@ROOT@', shlex.quote(str(r))).replace('@REAL@', shlex.quote(REAL_TMUX))
                            .replace('@LOGS@', shlex.quote(str(self.evidence.folder))))
            path.chmod(0o755)
        if self.agent == 'kimi':
            path = r / 'home/.local/bin/node'
            path.write_text(KIMI_NODE.replace('@LOGS@', shlex.quote(str(self.evidence.folder)))
                            .replace('@VALUE@', shlex.quote(json.dumps(kimi_inspection()))))
            path.chmod(0o755)
            fd = r / 'home/.local/bin/fd'
            fd.write_text(KIMI_FD.replace('@LOGS@', shlex.quote(str(self.evidence.folder))))
            fd.chmod(0o755)
            image = r / 'kimi-native'
            shutil.copyfile(self.native, image)
            image.chmod(0o700)
            import hashlib
            with self.native.open('rb') as source, image.open('rb') as copied:
                self.native_sha256 = hashlib.file_digest(source, 'sha256').hexdigest()
                if hashlib.file_digest(copied, 'sha256').hexdigest() != self.native_sha256:
                    raise AssertionError('the private Kimi image differs from the pinned download')
            self.native = image.resolve()
        # The wrappers log straight into the evidence folder, so their logs survive a
        # run that is killed; nothing copies them at the end.
        for log in ('tmux-calls.log', 'refused-calls.log'):
            (self.evidence.folder / log).touch()
        (r / 'home/.local/bin' / self.agent).symlink_to(self.native)
        home = r / 'home'
        self.env = {
            'PATH': f'{r}/bin:{home}/.local/bin:{SYSTEM_PATH}', 'HOME': str(home), 'USER': FIXTURE_USER,
            'LOGNAME': FIXTURE_USER, 'SHELL': '/bin/bash', 'LANG': 'C.UTF-8', 'TERM': 'xterm-256color',
            'XDG_CONFIG_HOME': f'{home}/.config', 'XDG_DATA_HOME': f'{home}/.local/share', 'XDG_CACHE_HOME': f'{home}/.cache',
            'XDG_STATE_HOME': f'{home}/.local/state', 'XDG_RUNTIME_DIR': f'{r}/run', 'HGS_STATE_DIR': f'{r}/state',
            'HGS_CONFIG_DIR': f'{r}/cfg', 'HGS_PEERS': '', 'TMUX_TMPDIR': f'{r}/tmux', 'TMPDIR': f'{r}/tmp',
            'DBUS_SESSION_BUS_ADDRESS': f'unix:path={r}/run/no-bus', 'DISABLE_AUTOUPDATER': '1',
            'CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC': '1', 'KIMI_CODE_NO_AUTO_UPDATE': '1', 'KIMI_DISABLE_TELEMETRY': '1',
            'GIT_CONFIG_NOSYSTEM': '1', 'PYTHONDONTWRITEBYTECODE': '1', 'HGS_TEST_BIN': HGS,
        }

    def preflight(self):
        r, env, failures = str(self.root), self.env, []
        def inside(p):
            return os.path.realpath(p).startswith(r + '/')
        for key in ('HOME', 'XDG_CONFIG_HOME', 'XDG_DATA_HOME', 'XDG_CACHE_HOME', 'XDG_STATE_HOME', 'XDG_RUNTIME_DIR',
                    'HGS_STATE_DIR', 'HGS_CONFIG_DIR', 'TMUX_TMPDIR', 'TMPDIR'):
            if not inside(env[key]):
                failures.append(key)
        for key in ('TMUX', 'TMUX_PANE', 'DISPLAY', 'WAYLAND_DISPLAY', 'ZERUS_CLIENT_DESKTOP_SERVICE',
                    'HGS_SESSION', 'HGS_RUN_ID', 'HGS_SESSION_ID'):
            if key in env:
                failures.append('present ' + key)
        account = pwd.getpwuid(os.getuid()).pw_name
        if env.get('USER') != FIXTURE_USER or env.get('LOGNAME') != FIXTURE_USER:
            failures.append('USER and LOGNAME are not the fixture account name')
        named = sorted(key for key, value in env.items() if value == account)
        if named:
            failures.append('the runner account name is the value of ' + ','.join(named))
        for key in ('TMUX_TMPDIR', 'XDG_RUNTIME_DIR'):
            if stat.S_IMODE(os.stat(env[key]).st_mode) != 0o700:
                failures.append('mode ' + key)
        if os.path.exists(os.path.join(env['XDG_RUNTIME_DIR'], 'systemd/private')):
            failures.append('user manager in the runtime folder')
        for name in ('tmux', 'ssh', 'scp', 'systemd-run', 'hgs'):
            if shutil.which(name, path=env['PATH']) != f'{r}/bin/{name}':
                failures.append('resolves ' + name)
        if self.agent == 'kimi':
            for name in ('node', 'fd'):
                expected = Path(env['HOME']) / '.local/bin' / name
                if shutil.which(name, path=env['PATH']) != str(expected):
                    failures.append('resolves ' + name)
        sock = Path(env['HGS_STATE_DIR']) / 'desktop/service.sock'
        if len(str(sock).encode()) >= 104:
            failures.append('service socket path length')
        if not (os.path.isabs(HGS) and os.access(HGS, os.X_OK)):
            failures.append('HGS_TEST_BIN is not an absolute executable')
        self.evidence.write('preflight.json', dict(root=r, failures=failures))
        if failures:
            raise AssertionError('Teams fixture preflight failed: ' + ', '.join(failures))

    # ---- commands --------------------------------------------------------------
    def hgs(self, *argv, stdin=None, timeout=60, ok=True):
        run = subprocess.run([HGS, *argv], input=stdin, env=self.env, capture_output=True, text=True, timeout=timeout)
        self.evidence.append('hgs-calls.jsonl', dict(at=time.time(), argv=list(argv), exit=run.returncode,
                                                     stdout=run.stdout[:8000], stderr=run.stderr[:2000]))
        if ok and run.returncode != 0:
            raise AssertionError(f'hgs {" ".join(argv[:3])} exited {run.returncode}: {run.stdout[-600:]} {run.stderr[-600:]}')
        return run

    def json(self, *argv, **kw):
        return json.loads(self.hgs(*argv, **kw).stdout)

    def wait(self, label, predicate, bound, step=1.0):
        start = now()
        last = None
        while True:
            last = predicate()
            if last:
                break
            if now() - start > bound:
                break
            time.sleep(step)
        row = dict(wait=label, case=self.case, seconds=round(now() - start, 2), bound=bound, ok=bool(last))
        self.waits.append(row)
        self.evidence.append('waits.jsonl', row)
        print('ZERUS_TEAMS_WAIT=' + json.dumps(row), flush=True)
        if not last:
            raise AssertionError(f'{label}: not reached within {bound} s')
        return last

    def script(self, name, body):
        path = self.root / 'scripts' / (name + '.sh')
        path.write_text('#!/bin/bash\nset -u\nH="$HGS_EXECUTABLE"\n' + body)
        return path

    # ---- bring-up --------------------------------------------------------------
    def start_model(self):
        self.model = ModelServer(self.root / 'model', self.root / 'scripts', self.evidence)
        threading.Thread(target=self.model.serve_forever, daemon=True).start()
        self.base = f'http://127.0.0.1:{self.model.server_address[1]}'

    def project(self):
        proj = self.root / 'work/proj'
        proj.mkdir(parents=True)
        git = ['git', '-C', str(proj), '-c', 'user.name=Fixture', '-c', 'user.email=fixture@example.invalid']
        subprocess.run(git[:3] + ['init', '-q', '-b', 'main'], env=self.env, check=True)
        (proj / 'README.md').write_text('Teams fixture project.\n')
        subprocess.run(git + ['add', 'README.md'], env=self.env, check=True)
        subprocess.run(git + ['commit', '-q', '-m', 'Fixture start'], env=self.env, check=True)
        self.commit = subprocess.check_output(git[:3] + ['rev-parse', 'HEAD'], env=self.env, text=True).strip()
        self.machine = self.json('swarm', 'hello')['node_id']
        self.hgs('swarm', 'initialize', stdin=json.dumps({'projects': [dict(
            id='teams-fixture', name='Teams fixture', color='#123456', sessions=[],
            folders=[dict(id='teams-fixture-folder', machine=self.machine, path=str(proj), name='')])]}))
        self.proj = proj

    def claude_home(self, home):
        home.mkdir(parents=True, exist_ok=True)
        (home / '.claude.json').write_text(json.dumps(dict(
            hasCompletedOnboarding=True, hasSeenAutoDefaultNudge=True, theme='dark', bypassPermissionsModeAccepted=True,
            autoUpdates=False, customApiKeyResponses=dict(approved=[FIXTURE_KEY], rejected=[]))))
        (home / 'settings.json').write_text(json.dumps({'env': {
            'ANTHROPIC_BASE_URL': self.base, 'ANTHROPIC_API_KEY': FIXTURE_KEY, 'DISABLE_AUTOUPDATER': '1',
            'CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC': '1'}}))

    def codex_common(self):
        return ('check_for_update_on_startup=false\ncli_auth_credentials_store="file"\n'
                f'chatgpt_base_url="{self.base}/backend-api/"\n[features]\nplugins=false\n')

    def accounts(self):
        self.account = 'zt-' + self.agent
        self.hgs('account', 'add', self.account, '--provider', self.agent, '--label', 'Teams fixture')
        home = Path(self.env['HGS_CONFIG_DIR']) / 'accounts' / self.account
        home.mkdir(parents=True, exist_ok=True)
        own = self.root / 'home'
        if self.agent == 'claude':
            self.claude_home(home)
            # The always-listed native account (HOME/.claude) is inspected too.
            self.claude_home(own / '.claude')
            (own / '.claude.json').write_text((own / '.claude/.claude.json').read_text())
        elif self.agent == 'codex':
            (home / 'config.toml').write_text(
                'model="gpt-6.1-sol"\nmodel_provider="fixture"\n' + self.codex_common().replace('[features]', '[features]', 1) +
                '[model_providers.fixture]\nname="Fixture"\n'
                f'base_url="{self.base}/v1"\nwire_api="responses"\nrequires_openai_auth=true\n'
                'request_max_retries=0\nstream_max_retries=0\n')
            native_home = own / '.codex'
            native_home.mkdir(parents=True, exist_ok=True)
            (native_home / 'config.toml').write_text(self.codex_common())
            # A key login the way a person makes one: Codex's own command.
            login = subprocess.run([str(self.native), 'login', '--with-api-key'], input=FIXTURE_KEY + '\n',
                                   env=dict(self.env, CODEX_HOME=str(home)), capture_output=True, text=True, timeout=60)
            if login.returncode != 0:
                raise AssertionError('codex login --with-api-key failed: ' + login.stderr[-500:])
        else:
            for kimi_home in (home, own / '.kimi-code'):
                kimi_home.mkdir(parents=True, exist_ok=True)
                (kimi_home / 'config.toml').write_text(
                    'default_model = "fixture"\ntelemetry = false\n'
                    f'[providers.fixture]\ntype = "openai"\nbase_url = "{self.base}/v1"\napi_key = "{FIXTURE_KEY}"\n'
                    '[models.fixture]\nprovider = "fixture"\nmodel = "fixture"\nmax_context_size = 32768\n'
                    '[loop_control]\nmax_attempts_per_step = 1\n')
                (kimi_home / 'tui.toml').write_text('[upgrade]\nauto_install = false\n')
            (home / 'credentials').mkdir(exist_ok=True)
            (home / 'credentials/kimi-code.json').write_text(json.dumps(dict(access_token='fixture-token', expires_at=1)))
            self.env['KIMI_CODE_BASE_URL'] = self.base + '/kimi-platform'
        self.account_home = home
        # Accounts a launch may choose: those whose home holds the fixture's key.
        self.fixture_accounts = {self.account} | ({'native-claude'} if self.agent == 'claude' else set())
        self.evidence.write('account-inspect.json', self.json('account', 'inspect', self.account, timeout=90))

    def bundle(self):
        role = dict(id='zt-dev', name='Fixture member', agent=self.agent, prompt='Fixture member; run the fixture steps.')
        if self.agent == 'claude':
            role.update(model='claude-sonnet-4-6', effort='high')
        elif self.agent == 'codex':
            role.update(model='gpt-6.1-sol', effort='high')
        if self.pinned:
            role['account'] = {'mode': 'named', 'account': self.pinned}
        return {'template': {'id': 'zt-' + self.agent, 'name': 'Teams fixture ' + self.agent, 'description': 'One member.',
                             'lead': {'agent': self.agent}, 'roster': [{'role': 'zt-dev', 'count': self.count}],
                             'default_pipeline': 'zt-direct', 'settings': {}},
                'roles': [role], 'pipelines': [{'id': 'zt-direct', 'name': 'Direct', 'implementer': 'zt-dev'}]}

    def create_team(self):
        preview = self.json('team', 'preview', 'on', '--json', ok=False)
        if preview.get('ok') is not True:
            raise AssertionError('The Teams preview was refused; run the suite from a person terminal or hosted CI, '
                                 'outside live-agent ancestry: ' + json.dumps(preview)[:800])
        self.json('team', 'template', 'import', '--json', stdin=json.dumps(self.bundle()))
        self.team = str(uuid.uuid4())
        reply = self.json('team', 'create', '--template', 'zt-' + self.agent, '--json', stdin=json.dumps({
            'request_id': str(uuid.uuid4()), 'team': self.team, 'expected_revision': 0, 'project': 'teams-fixture',
            'name': 'Teams fixture', 'goal': 'Real-agent contract.', 'home': self.machine,
            'integration_branch': 'zerus/team/fixture-' + self.agent, 'base_commit': self.commit,
            'delegations': self.delegations}))
        if reply.get('ok') is not True:
            raise AssertionError('team create: ' + json.dumps(reply)[:800])

    def start_service(self):
        log = (self.evidence.folder / 'desktop-serve.log').open('w')
        self.service = subprocess.Popen([HGS, 'desktop', 'serve', '--background'], env=self.env,
                                        stdin=subprocess.DEVNULL, stdout=log, stderr=log)
        self.wait('desktop service running', lambda: self.hgs('desktop', 'status', '--json', ok=False).returncode == 0
                  and json.loads(self.hgs('desktop', 'status', '--json', ok=False).stdout).get('running'), BOUND['service'])

    @staticmethod
    def listed(reply, key, what):
        """The list `key` of a CLI reply. A reply without it is an error of the reader,
        never an empty list, so a check that nothing is there cannot pass on a renamed key."""
        value = reply.get(key) if isinstance(reply, dict) else None
        if not isinstance(value, list):
            raise AssertionError(f'{what} reply has no {key} list: {json.dumps(reply)[:300]}')
        return value

    def status(self):
        return self.json('team', 'status', '--team', self.team, '--json')

    def members(self):
        return {m['id']: m for m in self.listed(self.status(), 'members', 'team status')}

    def rows(self):
        found = []
        def walk(value):
            if isinstance(value, dict):
                if 'run_id' in value and 'name' in value:
                    found.append(value)
                for child in value.values():
                    walk(child)
            elif isinstance(value, list):
                for child in value:
                    walk(child)
        walk(self.json('ls', '--json'))
        return found

    def session_of(self, member):
        key = (member.get('session') or {}).get('session_id') or ''
        launch = key.split(':', 1)[1] if key.startswith('launch_id:') else ''
        for row in self.rows():
            if launch and (row.get('launch_id') == launch or str(row.get('name', '')).endswith('/team-' + launch)):
                return dict(name=row['name'], launch=launch)
        return None

    def bring_up(self):
        self.build()
        self.preflight()
        self.census = Census(self.evidence, ALLOWED_HOSTS[self.agent])
        self.census.start()
        self.start_model()
        self.project()
        self.accounts()
        for account in self.pretrust:
            self.trust_main_checkout(account)
        self.create_team()
        self.start_service()
        bound = self.wait('lead and members bound by launch ID', lambda: (lambda m: m if len(m) == 1 + self.count and all(
            (x.get('session') or {}).get('session_id') for x in m.values()) else None)(self.members()), BOUND['bound'], 3)
        for member_id, member in bound.items():
            self.sessions[member_id] = self.wait(f'{member_id} session listed', lambda m=member: self.session_of(m),
                                                 BOUND['listed'], 2)
            self.sessions[member_id]['member'] = member
        self.census.roots.add(self.tmux_server())
        for member_id in bound:
            self.answer_startup(member_id)

    def trust_main_checkout(self, account):
        """The person trusts the team's main checkout on one account before the team
        starts: a plain session there on that account, its trust card answered through
        hgs answer, then the session ended by Terminate."""
        if not getattr(self, 'main_project', None):
            self.hgs('project', 'add', 'trusted-main', str(self.root / 'work/proj'))
            self.main_project = 'trusted-main'
        self.hgs('account', 'default', account)
        tag = 'trust-' + account
        self.hgs(self.agent, self.main_project, '-n', tag, '-d', timeout=120)
        row = self.wait(f'the plain session on {account} is listed', lambda: next(
            (r for r in self.rows() if str(r.get('name', '')).endswith('/' + tag)), None), BOUND['listed'], 2)
        key = 'plain-' + account
        self.sessions[key] = dict(name=row['name'], member={})
        view = self.answer_startup(key)
        self.hgs('terminate', row['name'], '--expected-run-id', view.get('run_id'), timeout=120)
        del self.sessions[key]

    def answer_question(self, member_id, card):
        """The person answers an ordinary question card in full: the first option of each
        question, or a short text where it offers none."""
        view = self.inspect(member_id)
        answers = []
        for item in card.get('questions') or []:
            options = item.get('options') or []
            answers.append(dict(question_id=item['id'], selected_option_ids=[options[0]['id']] if options else [],
                                text='' if options else 'Fixture answer.'))
        payload = dict(request_id=str(uuid.uuid4()), expected_run_id=view.get('run_id'),
                       expected_conversation_id=view.get('conversation_id') or '', question_id=card['question_id'],
                       expected_question_hash=card['question_hash'], answers=answers)
        return self.json('answer', self.sessions[member_id]['name'], '--json', stdin=json.dumps(payload))

    def handouts(self, node):
        """The members the service handed one node to, from its journal."""
        rows = self.listed(self.json('team', 'journal', '--team', self.team, '--limit', '200', '--json'), 'rows', 'team journal')
        return [ref[len('member:'):] for row in rows if row.get('kind') == 'handout_intent' and f'node:{node}' in (row.get('refs') or [])
                for ref in row.get('refs') or [] if ref.startswith('member:')]

    def tmux_server(self):
        out = subprocess.run(['tmux', 'display-message', '-p', '#{pid}'], env=self.env, capture_output=True, text=True)
        return int(out.stdout.strip()) if out.stdout.strip().isdigit() else os.getpid()

    def inspect(self, member_id):
        view = self.json('inspect', self.sessions[member_id]['name'], timeout=60)
        if view.get('tracked') is True and view.get('session_id'):
            self.sessions[member_id]['last_tracked'] = {key: view.get(key) for key in (
                'session_id', 'run_id', 'launch_id', 'conversation_id')}
        return view

    def answer_startup(self, member_id):
        """The person answers each startup card of this member until it is ready: through
        `hgs answer` when the card can be answered there (Claude and Kimi folder trust,
        Codex's Folder access and hooks review), otherwise in Terminal, where the person
        presses Enter on its highlighted first choice."""
        name = self.sessions[member_id]['name']
        answered = []

        def step():
            view = self.inspect(member_id)
            pending = view.get('pending_questions') or []
            live = view.get('runtime_state') == 'live' and view.get('process_state') == 'running'
            started = view.get('conversation_id') and view.get('activity') == 'idle'
            if not pending and live and (started or view.get('first_message_can_send')):
                return view
            for card in pending:
                self.capture_card(member_id, card)
                if card.get('source') in self.service_answers:
                    continue  # the standing confirmation's to answer
                wanted = self.after_refusal.get(card.get('source'))
                if wanted and not any(f'Member {member_id} needs your attention' in c.get('text', '') and wanted in c.get('text', '')
                                      for c in self.cards()):
                    continue  # the service has not refused it yet
                if card.get('question_id') in answered:
                    continue
                items = card.get('questions') or []
                options = items[0].get('options') if len(items) == 1 else []
                choice = options[0].get('label') if options else None
                if card.get('source') == 'startup_terminal_choice' and self.agent == 'kimi' and choice is None:
                    from test_kimi_approvals_native import trust_chooser_2_2
                    pane = self.panes.get((member_id, card.get('question_id')), '')
                    if trust_chooser_2_2(pane, view.get('cwd')):
                        choice = 'Trust and continue'
                if card.get('source') == 'startup_terminal_choice' and choice == 'Trust and continue':
                    subprocess.run(['tmux', 'send-keys', '-t', '=' + name + ':', 'Enter'], env=self.env, check=True)
                    answered.append(card.get('question_id'))
                    self.evidence.append('person-answers.jsonl', dict(member=member_id, source=card.get('source'),
                                                                       terminal=True, label=choice))
                    continue
                accept = [o for o in options or [] if re.match(r'(yes|trust)\b', o.get('label', ''), re.I)]
                if card.get('can_answer') and len(accept) == 1:
                    payload = dict(request_id=str(uuid.uuid4()), expected_run_id=view.get('run_id'),
                                   expected_conversation_id=view.get('conversation_id') or '',
                                   question_id=card['question_id'], expected_question_hash=card['question_hash'],
                                   answers=[dict(question_id=items[0]['id'], selected_option_ids=[accept[0]['id']], text='')])
                    reply = self.hgs('answer', name, '--json', stdin=json.dumps(payload), ok=False)
                    answered.append(card.get('question_id'))
                    self.evidence.append('person-answers.jsonl', dict(member=member_id, source=card.get('source'),
                                                                       label=accept[0]['label'], exit=reply.returncode))
            return None
        view = self.wait(f'{member_id} ready after the person answered its startup cards', step, BOUND['card'] + BOUND['ready'], 2)
        self.sessions[member_id].update(run=view.get('run_id'), conversation=view.get('conversation_id') or '')
        return view

    def capture_card(self, member_id, card):
        """The first sight of each startup card: the inspection's card and the pane as
        tmux holds it, with its size, so the real rendering is kept in the evidence."""
        key = (member_id, card.get('question_id'))
        if key in self.cards_seen:
            return
        target = '=' + self.sessions[member_id]['name'] + ':'
        pane = subprocess.run(['tmux', 'capture-pane', '-p', '-t', target], env=self.env, capture_output=True, text=True)
        size = subprocess.run(['tmux', 'display-message', '-p', '-t', target, '#{pane_width}x#{pane_height}'],
                              env=self.env, capture_output=True, text=True)
        number = len(self.cards_seen)
        self.cards_seen[key] = dict(member=member_id, source=card.get('source'), at=time.time(), size=size.stdout.strip(),
                                    card=card)
        self.evidence.write(f'startup-card-{number}-{member_id}.json', self.cards_seen[key])
        self.evidence.write(f'startup-card-{number}-{member_id}-pane.txt', pane.stdout)
        self.panes[key] = pane.stdout

    JOURNAL_BOUND = 500

    def journal(self):
        """Every attempt of the approval-audit journal, read through the product's own
        reader, hgs approval-log list (paged) and detail (src/approval_log.rs), so the
        store's place is the product's and a moved or renamed store is followed. Each row
        carries its question's source, ID and run and its categories. A reply or a row of
        any other shape fails the reader: an unknown shape is an error, never "no match".
        What this cannot catch: a product whose own writer and reader disagree on the
        store; and the product's reader reads a store that does not exist yet as empty."""
        entries, before = [], None
        while True:
            page = self.json('approval-log', 'list', stdin=json.dumps({} if before is None else {'before': before}))
            entries.extend(self.listed(page, 'entries', 'approval-log list'))
            if len(entries) > self.JOURNAL_BOUND:
                raise AssertionError(f'the audit journal holds more than {self.JOURNAL_BOUND} rows')
            before = page.get('next') if isinstance(page, dict) else None
            if before is None:
                break
        rows = []
        for entry in sorted(entries, key=lambda e: e.get('id', 0) if isinstance(e, dict) else 0):
            try:
                detail = self.json('approval-log', 'detail', stdin=json.dumps({'id': entry['id']}))
                question = detail['question']
                fields = {key: question[key] for key in ('source', 'question_id', 'run_id')}
                categories = entry['categories'] if entry['categories'] is not None else []
                if (not all(isinstance(value, str) for value in fields.values()) or not isinstance(detail['answers'], list)
                        or not isinstance(categories, list) or detail['id'] != entry['id']
                        or not isinstance(entry['session'], str) or not isinstance(entry['status'], str)):
                    raise TypeError('a field has an unexpected type')
            except (ValueError, KeyError, TypeError) as error:
                raise AssertionError(f'audit journal row {entry!r:.200} has a shape the reader does not know: {error!r}')
            rows.append(dict(id=entry['id'], at=entry.get('at'), session=entry['session'], provider=entry.get('provider'),
                             prompt=entry.get('prompt'), status=entry['status'], detail=detail.get('detail'),
                             source=fields['source'], question_id=fields['question_id'], run=fields['run_id'],
                             categories=categories, saved_question=question, scope=detail.get('scope')))
        return rows

    def audit_rows(self, session_name):
        """The journal's attempts of one session (see journal)."""
        return [row for row in self.journal() if row['session'] == session_name]

    def journal_summary(self):
        """The whole journal as (member, source, categories, status), sorted, for evidence
        and for assertions about everything it holds."""
        members = {session['name']: member_id for member_id, session in self.sessions.items()}
        return sorted((members.get(row['session'], row['session']), row['source'], tuple(row['categories']), row['status'])
                      for row in self.journal())

    def claude_trust_records(self):
        """Read-only, private root only: where Claude keeps its folder trust. Every
        .claude.json in the root, with its project keys and the trust flag only."""
        records = {}
        for path in sorted(self.root.rglob('.claude.json'))[:20]:
            try:
                projects = json.loads(path.read_text()).get('projects') or {}
            except (OSError, ValueError) as error:
                records[str(path)] = repr(error)
                continue
            records[str(path)] = {key: bool((value or {}).get('hasTrustDialogAccepted')) for key, value in projects.items()}
        return records

    def account_snapshot(self, home, limit=2000):
        """Read-only, private root only: every file under one account folder with its size
        and change time; for a JSON object, its top-level keys with a short hash of each
        value, and for .claude.json its project keys. Names and hashes only, no content."""
        import hashlib
        home = Path(home)
        out = {}
        for path in sorted(home.rglob('*'))[:limit]:
            if path.is_symlink() or not path.is_file():
                continue
            info = path.stat()
            entry = dict(size=info.st_size, mtime_ns=info.st_mtime_ns)
            if path.suffix == '.json' and info.st_size <= 1 << 20:
                try:
                    value = json.loads(path.read_text())
                except (OSError, ValueError):
                    value = None
                if isinstance(value, dict):
                    entry['keys'] = {key: hashlib.sha256(json.dumps(item, sort_keys=True).encode()).hexdigest()[:12]
                                     for key, item in value.items()}
                    if isinstance(value.get('projects'), dict):
                        entry['projects'] = sorted(value['projects'])
                        # Each project entry's own keys, names and value hashes only.
                        entry['project_entries'] = {
                            project: {key: hashlib.sha256(json.dumps(item, sort_keys=True).encode()).hexdigest()[:12]
                                      for key, item in (fields or {}).items()} if isinstance(fields, dict) else {}
                            for project, fields in value['projects'].items()}
            out[str(path.relative_to(home))] = entry
        return out

    @staticmethod
    def snapshot_diff(before, after):
        """Files added, removed and changed between two account snapshots; for a changed
        JSON object, the names of the keys added, removed or changed, and its project keys
        added or removed."""
        changed = {}
        for name in sorted(set(before) & set(after)):
            old, new = before[name], after[name]
            if (old['size'], old['mtime_ns']) == (new['size'], new['mtime_ns']):
                continue
            change = dict(size=[old['size'], new['size']])
            if 'keys' in old or 'keys' in new:
                a, b = old.get('keys', {}), new.get('keys', {})
                change['keys_added'] = sorted(set(b) - set(a))
                change['keys_removed'] = sorted(set(a) - set(b))
                change['keys_changed'] = sorted(k for k in set(a) & set(b) if a[k] != b[k])
            if 'projects' in old or 'projects' in new:
                change['projects_added'] = sorted(set(new.get('projects', [])) - set(old.get('projects', [])))
                change['projects_removed'] = sorted(set(old.get('projects', [])) - set(new.get('projects', [])))
                a, b = old.get('project_entries', {}), new.get('project_entries', {})
                change['project_keys'] = {
                    project: dict(added=sorted(set(b.get(project, {})) - set(a.get(project, {}))),
                                  removed=sorted(set(a.get(project, {})) - set(b.get(project, {}))),
                                  changed=sorted(k for k in set(a.get(project, {})) & set(b.get(project, {}))
                                                 if a[project][k] != b[project][k]))
                    for project in sorted(set(a) | set(b)) if a.get(project) != b.get(project)}
            changed[name] = change
        return dict(added=sorted(set(after) - set(before)), removed=sorted(set(before) - set(after)), changed=changed)

    def codex_trust_records(self):
        """Read-only, private root only: where Codex keeps its folder trust. Every
        config.toml in the root, with its [projects] keys and their trust_level only."""
        import tomllib
        records = {}
        for path in sorted(self.root.rglob('config.toml'))[:20]:
            try:
                projects = tomllib.loads(path.read_text()).get('projects') or {}
            except (OSError, ValueError) as error:
                records[str(path)] = repr(error)
                continue
            records[str(path)] = {key: (value or {}).get('trust_level') for key, value in projects.items()}
        return records

    def agent_home(self, member_id):
        """The account home a member runs in: a managed account, or the native one in HOME."""
        account = self.members()[member_id].get('account')
        if account and account.startswith('native-'):
            return Path(self.env['HOME'])
        return Path(self.env['HGS_CONFIG_DIR']) / 'accounts' / account

    def cards(self):
        return self.listed(self.json('team', 'cards', '--team', self.team, '--limit', '50', '--json'), 'rows', 'team cards')

    def pending(self, member_id):
        """The member's pending questions; a view without that list fails the reader."""
        return self.listed(self.inspect(member_id), 'pending_questions', 'inspect')

    def live_runs(self, launch, control):
        """The live sessions of one launch in `hgs ls`. Every listed session must say its
        runtime state, and the launch `control` (one known to be live) must be found live by
        the same reading: a positive control for "nothing of this launch is live". A launch
        or control that is not a non-empty string fails the reader: a missing launch must
        not satisfy "nothing of it runs"."""
        for value in (launch, control):
            if not isinstance(value, str) or not value:
                raise AssertionError(f'not a launch: {value!r}')
        rows = [row for row in self.rows() if row.get('launch_id') in (launch, control)]
        for row in rows:
            if not isinstance(row.get('runtime_state'), str):
                raise AssertionError(f'a listed session lacks its runtime state: {json.dumps(row)[:300]}')
        if not [row for row in rows if row['launch_id'] == control and row['runtime_state'] == 'live']:
            raise AssertionError(f'the control launch {control} is not found live in hgs ls')
        return [row['name'] for row in rows if row['launch_id'] == launch and row['runtime_state'] == 'live']

    # ---- person actions --------------------------------------------------------
    def send(self, member_id, text):
        view = self.inspect(member_id)
        request = dict(request_id=str(uuid.uuid4()), expected_run_id=view.get('run_id'),
                       expected_conversation_id=view.get('conversation_id') or '', text=text)
        reply = self.json('send', self.sessions[member_id]['name'], '--json', stdin=json.dumps(request))
        if reply.get('status') != 'submitted':
            raise AssertionError('send: ' + json.dumps(reply)[:600])
        return reply

    def worked(self, member_id, text):
        """The person sends one message and the member answers it: the model sees the
        message and the member's turn ends with a persisted conversation. Stop can
        precede the TUI accepting its next input, so also read the empty composer
        through the product before a native quit key is sent."""
        last = max([event.get('seq', 0) for event in self.inspect(member_id).get('events') or []] or [0])
        self.send(member_id, text)
        self.wait(f'{member_id} model received the person\'s message',
                  lambda: self.requests_with(lambda r: r.get('prompt') == text), BOUND['turn'], 1)
        def answered():
            view = self.inspect(member_id)
            if not view.get('conversation_id') or not any(event.get('type') == 'Stop' and event.get('seq', 0) > last
                                                         for event in view.get('events') or []):
                return None
            read = self.hgs('terminal', self.sessions[member_id]['name'], '--json', ok=False, stdin=json.dumps(dict(
                request_id=str(uuid.uuid4()), action='composer', expected_run_id=view['run_id'],
                expected_conversation_id=view['conversation_id'])))
            if read.returncode != 0:
                return None
            composer = json.loads(read.stdout)
            if composer['composer']['state'] != 'empty' or view.get('activity') != 'idle':
                return None
            self.evidence.write('before-native-exit-composer.json', composer)
            return view
        return self.wait(f'{member_id} answered the person\'s message', answered, BOUND['turn'], 2)

    def add_task(self, title, text):
        reply = self.json('task', 'add', '--project', 'teams-fixture', '--team', self.team, '--title', title, '--json',
                          stdin=text)
        task = reply.get('task') or reply
        return task['ref'], task['uuid']

    def node_of(self, uuid_):
        rows = self.listed(self.json('team', 'queue', '--team', self.team, '--limit', '200', '--json'), 'rows', 'team queue')
        return next((r for r in rows if (r.get('task') or {}).get('uuid') == uuid_), None)

    def requests_with(self, predicate):
        with self.model.lock:
            return [r for r in self.model.requests if predicate(r)]

    def process(self, member_id):
        """The native agent's PID and kernel start time from its session record."""
        record = self.record(member_id)
        pid = record.get('pid')
        ticks = self.start_ticks(pid)
        if ticks is None:
            # A process that is already gone has no identity to compare later.
            raise AssertionError(f'{member_id} process {pid} is not running when its identity is taken')
        return pid, ticks

    @staticmethod
    def start_ticks(pid):
        try:
            text = open(f'/proc/{pid}/stat').read()
            return int(text[text.rindex(')') + 2:].split()[19])
        except (OSError, ValueError, TypeError):
            return None

    def record(self, member_id):
        """The one live session record of the member's current run (an archive of the same
        run is not it); none or several fail the reader."""
        run = self.inspect(member_id).get('run_id')
        if not run:
            raise AssertionError(f'{member_id} has no current run')
        found = []
        for path in Path(self.env['HGS_STATE_DIR']).rglob('*.json'):
            try:
                data = json.loads(path.read_text())
            except (OSError, ValueError):
                continue
            if isinstance(data, dict) and data.get('run_id') == run and data.get('pid') and data.get('state') != 'archived':
                found.append((path, data))
        if len(found) != 1:
            raise AssertionError(f'{len(found)} live session records for {member_id} run {run}: '
                                 + ', '.join(str(path) for path, _ in found[:5]))
        return found[0][1]

    # ---- teardown --------------------------------------------------------------
    def owned_left(self):
        parents = Census.table()
        left = []
        root = str(self.root) + '/'
        for pid in parents:
            if pid == os.getpid():
                continue
            hit = False
            for key in ('cwd', 'exe'):
                try:
                    hit = hit or (os.readlink(f'/proc/{pid}/{key}') + '/').startswith(root)
                except OSError:
                    pass
            try:
                hit = hit or root.encode() in open(f'/proc/{pid}/environ', 'rb').read()
            except OSError:
                pass
            if hit:
                left.append(pid)
        return left

    def diagnose(self):
        """Bounded reads that explain a failed step: owner cards, waiting rows, members."""
        if not self.team:
            return
        for member, session in self.sessions.items():
            target = '=' + session['name'] + ':'
            for suffix, args in (('pane', ['capture-pane', '-p', '-e', '-t', target]),
                                 ('cursor', ['display-message', '-p', '-t', target, '#{cursor_x},#{cursor_y}'])):
                run = subprocess.run(['tmux', *args], env=self.env, capture_output=True, text=True, timeout=10)
                self.evidence.write(f'diagnose-{member}-{suffix}.txt', run.stdout)
        for name, argv in (('cards', ('team', 'cards', '--team', self.team, '--limit', '50', '--json')),
                           ('waiting', ('team', 'waiting', '--limit', '50', '--json')),
                           ('status', ('team', 'status', '--team', self.team, '--json')),
                           ('journal', ('team', 'journal', '--team', self.team, '--limit', '100', '--json'))):
            try:
                self.evidence.write(f'diagnose-{name}.json', self.hgs(*argv, ok=False, timeout=30).stdout)
            except Exception as error:
                self.evidence.write(f'diagnose-{name}.json', str(error)[:500])

    VIEW_FIELDS = ('tracked', 'name', 'agent', 'cwd', 'run_id', 'session_id', 'launch_id', 'conversation_id', 'state',
                   'archive_id', 'pending_questions_error', 'runtime_state', 'process_state', 'allowed_actions', 'action_reasons')
    RECORD_FIELDS = ('archive_id', 'name', 'agent', 'cwd', 'run_id', 'session_id', 'launch_id', 'conversation_id',
                     'account_id', 'state', 'conversation_state', 'completion_source', 'completion_reason', 'exited_at',
                     'exit_code', 'termination_signal', 'session_end', 'error', 'expected_id', 'paused', 'pausing',
                     'terminating', 'launch_dir', 'worktree_cleanup')

    def observation(self):
        """Read-only, through the private state only: what the service's observation of a
        member reads (confirm/native/observation.rs), so a refused resume names its step:
        the inspection by the member's last session ID, the archive catalog rows of its
        name, each archive's exit fields and the archive's own inspection; then the
        lifecycle receipts of the member's session (the CLI's own refusal text) and
        whether its worktree folder still exists."""
        import sqlite3
        state = Path(self.env['HGS_STATE_DIR'])
        after = str(2 ** 63 - 1)

        def read(*argv):
            run = self.hgs(*argv, ok=False, timeout=60)
            try:
                value = json.loads(run.stdout)
            except ValueError:
                value = None
            if not isinstance(value, dict):
                return dict(exit=run.returncode, stdout=run.stdout[:500], stderr=run.stderr[-500:])
            return dict(exit=run.returncode, stderr=run.stderr[-500:], pending_questions=len(value.get('pending_questions') or []),
                        **{key: value.get(key) for key in self.VIEW_FIELDS})

        catalog = state / 'archive-catalog-v1.sqlite3'
        receipts = []
        folder = state / 'action_receipts'
        if folder.is_dir() and not folder.is_symlink():
            for path in sorted(folder.glob('*.json'))[:50]:
                if path.is_file() and not path.is_symlink() and path.stat().st_size <= 64 << 10:
                    receipt = json.loads(path.read_text())
                    receipts.append({key: receipt.get(key) for key in (
                        'request_id', 'name', 'run_id', 'conversation_id', 'archive_id', 'status', 'at', 'error',
                        'error_source', 'result_target')})
        out = {}
        for member_id, session in self.sessions.items():
            last = session.get('last_tracked') or {}
            worktree = (session.get('member') or {}).get('worktree') or ''
            item = out[member_id] = dict(name=session['name'], last_tracked=last, archives=[], worktree=dict(
                path=worktree, exists=bool(worktree) and Path(worktree).is_dir(),
                git_link=bool(worktree) and (Path(worktree) / '.git').is_file()),
                action_receipts=[r for r in receipts if r.get('name') == session['name']])
            if last.get('session_id'):
                item['by_session_id'] = read('inspect', '--session-id', last['session_id'], '--after', after,
                                             '--skip-processes')
            rows = []
            if catalog.is_file():
                db = sqlite3.connect(f'file:{catalog}?mode=ro', uri=True, timeout=1)
                try:
                    rows = db.execute('SELECT id, run, conversation FROM archive_rows WHERE name=? LIMIT 5',
                                      (session['name'],)).fetchall()
                finally:
                    db.close()
            for archive_id, run, conversation in rows:
                entry = dict(id=archive_id, run=run, conversation=conversation)
                path = state / 'archives' / f'{archive_id}.json'
                if path.is_file() and not path.is_symlink() and path.stat().st_size <= 1 << 20:
                    record = json.loads(path.read_text())
                    entry['record'] = {key: record.get(key) for key in self.RECORD_FIELDS}
                    entry['record'].update(has_supervisor='supervisor' in record,
                                           worktree_owner=(record.get('worktree_owner') or {}).get('id'))
                entry['view'] = read('inspect', session['name'], '--archive', archive_id, '--after', after,
                                     '--skip-processes')
                item['archives'].append(entry)
            item['worktree_maintenance'] = self.maintenance(worktree)
        return out

    def maintenance(self, worktree):
        """Read-only, private state only: the worktree lifecycle's own records of one
        member checkout: its policy, its queue rows, its last sweep result and the
        ownership receipt that names the checkout."""
        import sqlite3
        state = Path(self.env['HGS_STATE_DIR'])
        folder = state / 'worktree-maintenance'
        out = dict(policy_file=None, checkouts=[], removals=[], last_result=None, owned_receipts=[])

        def small(path, limit=64 << 10):
            if path.is_file() and not path.is_symlink() and path.stat().st_size <= limit:
                try:
                    return json.loads(path.read_text())
                except ValueError:
                    return path.read_text()[:2000]
            return None
        out['policy_file'] = small(folder / 'policy.json')
        out['last_result'] = small(folder / 'last-result.json')
        queue = folder / 'queue.sqlite3'
        if worktree and queue.is_file() and not queue.is_symlink():
            db = sqlite3.connect(f'file:{queue}?mode=ro', uri=True, timeout=1)
            try:
                for table in ('checkouts', 'removals'):
                    cursor = db.execute(f'SELECT * FROM {table} WHERE path=? LIMIT 5', (worktree,))
                    columns = [c[0] for c in cursor.description]
                    out[table] = [dict(zip(columns, row)) for row in cursor]
            finally:
                db.close()
        cache = state / 'worktree-cache'
        if worktree and cache.is_dir() and not cache.is_symlink():
            for path in sorted(cache.glob('owned-*.json'))[:50]:
                value = small(path, 16 << 10)
                if value is not None and worktree in json.dumps(value):
                    out['owned_receipts'].append(dict(file=path.name, value=value))
        return out

    def person_restore(self, member_id, archive):
        """The person's own Restore of one archive: the request the desktop sends through
        `hgs session-action`, with a new request ID, and the member's view after it."""
        name = self.sessions[member_id]['name']
        request = dict(request_id=str(uuid.uuid4()), action='restore', expected_run_id=archive['run'],
                       expected_conversation_id=archive['conversation'], archive_id=archive['id'])
        run = self.hgs('session-action', name, '--json', stdin=json.dumps(request), ok=False, timeout=120)
        try:
            receipt = json.loads(run.stdout)
        except ValueError:
            receipt = run.stdout[:2000]
        time.sleep(5)
        view = self.hgs('inspect', name, ok=False, timeout=60)
        try:
            after = {key: json.loads(view.stdout).get(key) for key in ('tracked', 'run_id', 'conversation_id',
                                                                      'runtime_state', 'process_state', 'state')}
        except ValueError:
            after = view.stdout[:500]
        return dict(request=request, exit=run.returncode, receipt=receipt, stderr=run.stderr[-1000:], view_after=after)

    def fleet_rows(self):
        """Read-only: the private service's fleet rows of this team's runs, the fields the
        teams actor reads to decide that a member is free (actor/availability.rs)."""
        import sqlite3
        path = Path(self.env['HGS_STATE_DIR']) / 'desktop/state.sqlite3'
        if not path.is_file():
            return {}
        out = {}
        db = sqlite3.connect(f'file:{path}?mode=ro', uri=True, timeout=1)
        try:
            for member_id, session in self.sessions.items():
                run = session.get('run')
                if not run:
                    continue
                rows = db.execute('SELECT generation, json FROM desktop_catalog_rows WHERE key=? ORDER BY generation DESC LIMIT 2',
                                  ('run:' + run,)).fetchall()
                out[member_id] = []
                for generation, text in rows:
                    row = json.loads(text)
                    out[member_id].append(dict(generation=generation, keys=sorted(row), **{k: row.get(k) for k in (
                        'state', 'activity', 'phase', 'process_work', 'subagents_active', 'optional_questions',
                        'launch_id', 'run_id', 'tracked', 'process_state', 'recovery', 'subagent_active_count')}))
        finally:
            db.close()
        return out

    def teardown(self):
        problems = []
        self.diagnose()
        try:
            self.evidence.write('diagnose-fleet-rows.json', self.fleet_rows())
        except Exception as error:
            self.evidence.write('diagnose-fleet-rows.json', str(error)[:500])
        for name in ('teams/member-launch-v1.sqlite3',):
            try:
                import sqlite3
                path = Path(self.env['HGS_STATE_DIR']) / name
                if path.is_file():
                    db = sqlite3.connect(f'file:{path}?mode=ro', uri=True, timeout=1)
                    tables = {}
                    for (table,) in db.execute("SELECT name FROM sqlite_master WHERE type='table'"):
                        cursor = db.execute(f'SELECT * FROM "{table}" LIMIT 40')
                        cols = [c[0] for c in cursor.description]
                        tables[table] = [dict(zip(cols, [v if not isinstance(v, bytes) else v.hex()[:120] for v in r])) for r in cursor]
                    db.close()
                    self.evidence.write('diagnose-' + name.replace('/', '-') + '.json', tables)
            except Exception as error:
                self.evidence.write('diagnose-ledger-error.txt', str(error)[:500])
        try:
            self.evidence.write('diagnose-observation.json', self.observation())
        except Exception as error:
            self.evidence.write('diagnose-observation.json', str(error)[:500])
        if self.agent in ('claude', 'codex'):
            name = self.agent + '-trust-records.json'
            try:
                worktrees = {member_id: (session.get('member') or {}).get('worktree') for member_id, session in self.sessions.items()}
                records = self.claude_trust_records() if self.agent == 'claude' else self.codex_trust_records()
                self.evidence.write(name, dict(worktrees=worktrees, records=records))
            except Exception as error:
                self.evidence.write(name, str(error)[:500])
        for member_id, session in list(self.sessions.items()):
            try:
                view = self.inspect(member_id)
                if view.get('runtime_state') == 'live':
                    self.hgs('terminate', session['name'], '--expected-run-id', view.get('run_id'), ok=False, timeout=90)
            except Exception as error:
                problems.append(f'terminate {member_id}: {error}')
        if self.service is not None and self.service.poll() is None:
            self.service.send_signal(signal.SIGTERM)
            try:
                code = self.service.wait(timeout=30)
                if code != 0:
                    problems.append(f'desktop service exited {code}')
            except subprocess.TimeoutExpired:
                self.service.kill()
                problems.append('desktop service did not stop within 30 s')
        subprocess.run(['tmux', 'kill-server'], env=self.env, capture_output=True)
        if self.model is not None:
            self.model.shutdown()
            self.model.server_close()
        try:
            left = self.wait('no owned process left', lambda: not self.owned_left() or None, BOUND['teardown'], 1)
        except AssertionError:
            left = None
            problems.append('owned processes left: ' + ','.join(map(str, self.owned_left()[:20])))
        census = self.census.finish() if self.census else {}
        calls = [l.split('\t') for l in (self.evidence.folder / 'tmux-calls.log').read_text().splitlines()]
        refused = (self.evidence.folder / 'refused-calls.log').read_text().splitlines()
        self.evidence.write('isolation.json', dict(tmux_calls=len(calls), tmux_refused=sum(c[1] == 'refused' for c in calls if len(c) > 1),
                                                    shim_calls=len(refused), census=census, problems=problems))
        if any(len(c) > 1 and c[1] == 'refused' for c in calls):
            problems.append('a tmux call named no private server')
        if refused:
            problems.append(f'{len(refused)} refused ssh/scp/systemd-run/hgs calls')
        if census.get('violations'):
            problems.append('non-allow-listed network peers: ' + json.dumps(census['violations'][:5]))
        if left is not None and not problems:
            shutil.rmtree(self.root, ignore_errors=True)
        return problems
