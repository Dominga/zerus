#!/usr/bin/env python3
"""Real Claude Code and Codex in the Windows session host, keyless (spec M7).

Windows only. HGS_TEST_BIN names the hgs.exe under test (the release build in CI);
HGS_CLAUDE_TEST_BIN and HGS_CODEX_TEST_BIN name the native executables taken from the
pinned win32-x64 npm packages (scripts/ci/agent-compatibility.json), and
HGS_TEST_CONPTY_DIR, when set, the pinned ConPTY pair that the portable zip ships
(scripts/fetch-conpty.py). A class skips only off Windows or without its executable or
HGS_TEST_BIN: scripts/ci/agent-compatibility.py counts a skip as a failure, and a
skipped native suite is no compatibility evidence. Whatever the agents do wrong is a
failure, never a skip.

Each case enables local sessions in a temporary profile as tests/test_windows_lifecycle.py
does: `hgs session-host install`, run from a portable copy of the build (beside the ConPTY
pair when given, as in the zip), stages the stable CLI, a host image and the marker there,
and the first launch starts the host from that image. The agent's executable is linked
into the profile's `.local\\bin`, which hgs puts first on PATH (src/platform/windows.rs
`prepare_path`; agents are found on that PATH, src/state/backend/host.rs), so only that
copy runs. Its home (CLAUDE_CONFIG_DIR, CODEX_HOME), where hgs installs its hooks (spec
D12), is the case's own, as are APPDATA, LOCALAPPDATA and TEMP; Codex still resolves
`~` to the real profile (FOLDERID_Profile), see `project_parent`. The agents reach only
tests/model_api_fixture.py on 127.0.0.1, run with `python -I -X utf8`, with a dummy key;
any other HTTP(S) request goes to a dead local proxy, and updates, telemetry and other
nonessential traffic are off. Codex runs with full access and no approvals, as in spike
S6, so it never needs its Windows sandbox; the CI job never extracts the sandbox setup
program, and Codex never runs it.

Sessions are driven only through hgs, as the GUI drives them: a tracked launch,
`ls --json`, `inspect`, `send --json`, `answer --json`, `clear-context --json`, `pause`,
`resume -d` and `terminate`. What an agent accepted is read from its own transcript,
which hgs never writes. Each case covers one area for one agent:

- launch: the tracked launch reaches the composer, its hooks confirm the conversation in
  the listing, `inspect` and the journal; `hgs send` delivers a plain, a non-ASCII, a
  backslash-terminated and a multiline message, each exactly once, and a replayed
  request delivers nothing.
- question: the fake model asks (Claude's AskUserQuestion, Codex's
  request_user_input_async) and `hgs answer` answers from Activity.
- approval: Claude's Write permission prompt, and Codex's review of the hooks hgs
  installed, answered with `hgs answer`.
- answer channel (Claude): the PermissionRequest hook hgs registers in exec form takes
  the question's answers and Bash approvals (once, feedback, No), a Bash panel too tall
  for the host's 80x24 too; Terminal answers and a killed hook end it, and the verified
  screen takes over only from a hook that took nothing.
- clear: `hgs clear-context` starts a new conversation in the same agent process. For
  Codex, hgs must first not take the header above a message for its reset panel
  (zerus-g9dd, which a project folder short enough to show whole exposes).
- lifecycle: `hgs pause`, `hgs resume -d` on the same conversation, then `hgs terminate`.

Model and effort pickers and compaction are not reviewed on Windows and stay out. Every
wait has a deadline; a failure shows the session's screen, the host's log and the fake
model's requests. Each case stops what it started, its sessions, the host and the fake
model, and waits for every agent and supervisor through handles opened while they ran,
and until nothing holds its executables, before its folders go.
"""
from contextlib import closing
import hashlib
import json
import os
from pathlib import Path
import shutil
import sqlite3
import subprocess
import sys
import tempfile
import time
import tomllib
import unittest
import uuid

import model_api_fixture as api
import test_windows_host as host
from codex_resume_diagnostics import ResumeDiagnostics

TESTS = Path(__file__).resolve().parent
EXE = ".exe" if os.name == "nt" else ""
HGS_SET = bool(os.environ.get("HGS_TEST_BIN"))
CLAUDE = os.environ.get("HGS_CLAUDE_TEST_BIN")
CODEX = os.environ.get("HGS_CODEX_TEST_BIN")
CONPTY = os.environ.get("HGS_TEST_CONPTY_DIR")
KEY = "fixture-key"
DEAD_PROXY = "http://127.0.0.1:9"

# Claude 2.1.319's shared permission dialog rejects input for 150 ms after
# first paint. A complete hook card is an observation, not an input-ready ACK.
CLAUDE_PERMISSION_INPUT_GUARD = 0.150


def wait_claude_permission_guard(card, read, observed, now=None, pause=None):
    """Wait out the native window on this original card, before ONE key.

    This does not claim to explain the historical Windows missed Enter. An
    unchanged prompt after that key still fails; it never authorizes replay.
    """
    now = now or time.monotonic
    pause = pause or time.sleep
    fields = ('question_id', 'question_hash', 'run_id', 'conversation_id',
              'tool_call_id', 'agent_id', 'source')
    until = observed + CLAUDE_PERMISSION_INPUT_GUARD
    while True:
        current = read()
        if (not current or not current.get('panel_verified') or not current.get('can_answer')
                or any(current.get(field) != card.get(field) for field in fields)):
            raise AssertionError('The original Claude permission panel changed before Terminal input')
        remaining = until - now()
        if remaining <= 0:
            return
        pause(min(remaining, 0.040))

# Seconds an agent may take to start, finish a turn, pause or exit: Claude Code starts
# in about 9 s under ConPTY on the development PC (spike S6), and a hosted runner scans
# a freshly linked executable on its first start.
WAIT = 120
# What the native test runner and the user's shell may export that no agent of a case
# may inherit: other agent accounts, providers and homes, terminal identities, CI tokens.
CLEARED_PREFIXES = ("CLAUDE", "ANTHROPIC_", "CODEX_", "OPENAI_", "KIMI_", "VSCODE_", "TERM_", "WT_", "CONEMU",
                    "ACTIONS_")
CLEARED = {"COLORTERM", "FORCE_COLOR", "NO_COLOR", "GITHUB_TOKEN", "GH_TOKEN", "HTTP_PROXY", "HTTPS_PROXY",
           "ALL_PROXY", "NO_PROXY", "NODE_OPTIONS",
           # A runner inside a Zerus session must not lend it to the case.
           "HGS_SESSION", "HGS_RUN_ID", "HGS_AGENT", "HGS_ACCOUNT_ID", "HGS_EXPECTED_ID", "HGS_REQUESTED_ID"}
# Messages `hgs send` must deliver exactly: plain, non-ASCII, ending in a backslash
# (which Claude reads as a line continuation before Enter), and multiline.
MESSAGES = ("Plain fixture message.", "Привет, ünïcödé ✓ 漢字.", "Continue the fixture\\",
            "Two fixture lines:\nC:\\fixture\\path and more.")


def path_forms(path):
    """The spellings an agent may key a folder by on Windows: separators and drive letter case."""
    native = str(path)
    forms = [native, native.replace("\\", "/")]
    if len(native) > 1 and native[1] == ":":
        forms += [native[0].lower() + native[1:], (native[0].lower() + native[1:]).replace("\\", "/")]
    return list(dict.fromkeys(forms))


def below(path, folder):
    """Whether `path` lies in `folder`, without case or 8.3 short names on Windows: a
    hosted runner's TEMP is `C:\\Users\\RUNNER~1\\…` below `C:\\Users\\runneradmin`."""
    path, folder = (os.path.normcase(os.path.realpath(value)) for value in (path, folder))
    return path == folder or path.startswith(folder.rstrip(os.sep) + os.sep)


def in_use(path):
    """Whether a process runs or loads the file at `path`: Windows then refuses to
    open it for writing, as tests/test_windows_host_conpty.py checks. Opening changes
    nothing; a read-only file is never taken for one in use."""
    try:
        with open(path, "r+b"):
            return False
    except FileNotFoundError:
        return False
    except PermissionError:
        return os.access(path, os.W_OK)


def project_parent():
    """Where a case's project folder goes. Codex prints a folder below the real profile
    (FOLDERID_Profile, not USERPROFILE) as `~\\…`, which hgs reads against the case's
    profile, so a project outside the real profile keeps its reset panel confirmable.
    The real profile is this process's USERPROFILE, which scripts/ci/agent-compatibility.py
    leaves unchanged for the suites. A hosted runner's TEMP lies inside the profile and
    RUNNER_TEMP outside it."""
    default = tempfile.gettempdir()
    profile = os.environ.get("USERPROFILE")
    runner = os.environ.get("RUNNER_TEMP")
    if profile and below(default, profile) and runner and os.path.isdir(runner) and not below(runner, profile):
        return runner
    return default


class NativeAgent:
    """One real agent in the session host of a temporary profile; the classes below name it."""

    AGENT = None
    BINARY = None
    # The agent's arguments after `--` in every launch.
    ARGS = ()
    # The glyph that starts the agent's composer row.
    COMPOSER = None
    CLEARS_WITH_KEYS = False
    # The flag that keeps an agent from asking to review the hooks hgs installs.
    NO_HOOK_REVIEW = None

    def setUp(self):
        # Handles on the agents and supervisors this case saw running, by (PID, start), and
        # its sessions, for the cleanup that HostCase registers.
        self.pinned, self.names, self.project_parent = {}, [], None
        super().setUp()
        # Agents and hgs compare folders as they print them: no 8.3 short names.
        self.root = self.root.resolve()
        self.state, self.home, self.work = (self.root / name for name in ("state", self.PROFILE, "work"))
        self.project_parent = Path(tempfile.mkdtemp(prefix="hgs-native-", dir=project_parent())).resolve()
        self.project = self.project_parent / "project"
        self.agent_home = self.root / f"{self.AGENT}-home"
        # The account a switch moved the session to, once the case made it.
        self.second_home = None
        self.api_root = self.root / "api"
        for folder in (self.project, self.agent_home, self.api_root, *(self.root / name for name in
                                                                         ("appdata", "localappdata", "temp"))):
            folder.mkdir(parents=True)
        self.env = {key: value for key, value in self.env.items()
                    if key.upper() not in CLEARED and not key.upper().startswith(CLEARED_PREFIXES)}
        self.env.update(APPDATA=str(self.root / "appdata"), LOCALAPPDATA=str(self.root / "localappdata"),
                        TEMP=str(self.root / "temp"), TMP=str(self.root / "temp"),
                        HGS_STATE_DIR=str(self.state), USERPROFILE=str(self.home), HOME=str(self.home),
                        HGS_SELF="test", HGS_PEERS="", HGS_TAB="0")
        self.api_url = f"http://127.0.0.1:{self.start_api()}"
        self.env.update(
            ANTHROPIC_BASE_URL=self.api_url, ANTHROPIC_API_KEY=KEY, OPENAI_API_KEY=KEY, FIXTURE_API_KEY=KEY,
            CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC="1", DISABLE_AUTOUPDATER="1", DISABLE_UPDATES="1",
            DISABLE_INSTALLATION_CHECKS="1", CLAUDE_CODE_DISABLE_OFFICIAL_MARKETPLACE_AUTOINSTALL="1",
            DISABLE_TELEMETRY="1", DISABLE_ERROR_REPORTING="1",
            HTTPS_PROXY=DEAD_PROXY, HTTP_PROXY=DEAD_PROXY, NO_PROXY="127.0.0.1,localhost")
        self.configure()
        self.install()
        self.place_agent()

    # The profile, the agent and the fake model.

    def start_api(self):
        """Start the fake model for this case; its port."""
        output = open(self.api_root / "output.txt", "wb")
        self.api = subprocess.Popen(
            [sys.executable, "-I", "-X", "utf8", str(TESTS / "model_api_fixture.py"), "--root", str(self.api_root),
             "--project", str(self.project)], cwd=self.api_root, stdin=subprocess.DEVNULL, stdout=output,
            stderr=subprocess.STDOUT, creationflags=host.HIDDEN)
        self.addCleanup(self.stop_api, output)
        port = self.api_root / "port"
        self.until("the model API fixture", lambda: port.is_file() or self.api.poll() is not None, timeout=30)
        if not port.is_file():
            self.fail("the model API fixture exited:\n" + (self.api_root / "output.txt").read_text(errors="replace"))
        return int(port.read_text())

    def stop_api(self, output):
        self.api.kill()
        self.api.wait(timeout=30)
        output.close()

    def install(self):
        """Enable local sessions in the profile from a portable copy of the build under test,
        beside the pinned ConPTY pair when one is given (spec D2, D6)."""
        for key in ("HGS_HOST_IMAGE", "HGS_HOST_ALLOW_UNSTAGED"):
            self.env.pop(key, None)
        portable = self.root / "portable"
        portable.mkdir()
        self.hgs_exe = portable / "hgs.exe"
        shutil.copy2(host.HGS, self.hgs_exe)
        if CONPTY:
            for name in ("conpty.dll", "OpenConsole.exe"):
                shutil.copy2(Path(CONPTY) / name, portable / name)
        self.hgs("session-host", "install", "--json")
        self.bin = self.home / ".local" / "bin"
        self.assertTrue((self.bin / "hgs.exe").is_file(), "the stable CLI is missing after install")

    def place_agent(self):
        """Link the pinned executable into the profile's `.local\\bin`, which hgs puts first on
        PATH; a copy where a link cannot be made."""
        target = self.bin / f"{self.AGENT}{EXE}"
        try:
            os.link(self.BINARY, target)
        except OSError:
            shutil.copy2(self.BINARY, target)

    def configure(self):
        """The agent's private home, as a user who has finished onboarding left it."""
        raise NotImplementedError

    def prepare_account(self, folder):
        """A second, signed-in account of the same agent in its own profile folder (U53)."""
        raise NotImplementedError

    # hgs and what it reports.

    def hgs(self, *args, input=None, rc=0):
        result = subprocess.run([str(self.hgs_exe), *args], env=self.env, cwd=self.work, input=input,
                                stdin=None if input is not None else subprocess.DEVNULL, capture_output=True,
                                text=True, encoding="utf-8", errors="replace", timeout=WAIT,
                                creationflags=host.HIDDEN)
        if rc is not None and result.returncode != rc:
            self.fail(f"hgs {' '.join(args)}: exit {result.returncode}\n{result.stdout}{result.stderr}"
                      + self.describe())
        return result

    def json(self, *args, input=None):
        return json.loads(self.hgs(*args, input=input).stdout)

    def rows(self):
        return {row["name"]: row for row in self.json("ls", "--json", "--local")["sessions"]}

    def row(self, name):
        return self.rows().get(name) or {}

    def inspect(self, name):
        return self.json("inspect", name)

    def record(self, name):
        """The session's live record (state directory), or {}."""
        path = self.state / (hashlib.sha256(name.encode()).hexdigest() + ".json")
        try:
            return json.loads(path.read_text(encoding="utf-8"))
        except (OSError, ValueError):
            return {}

    def journal(self, run_id):
        """The event types the journal holds for one run, in order."""
        path = self.state / "events.sqlite3"
        database = sqlite3.connect(f"file:{path.as_posix()}?mode=ro", uri=True, timeout=10)
        try:
            rows = database.execute("SELECT payload FROM events ORDER BY seq").fetchall()
        finally:
            database.close()
        events = [json.loads(payload) for payload, in rows]
        return [event.get("type") for event in events if event.get("run_id") == run_id]

    def capture(self, name):
        """The session's screen as the host draws it (UTF-8, whatever the code page)."""
        result = self.hgs("__host", "call", json.dumps({"op": "capture", "target": {"name": name}, "styled": False}),
                          rc=None)
        return json.loads(result.stdout)["ok"]["text"] if result.returncode == 0 else f"(no capture: {result.stderr})"

    def pane(self, name):
        """The host's pane facts, including where its caret is, in cells."""
        result = self.hgs("__host", "call", json.dumps({"op": "pane_info", "target": {"name": name}}), rc=None)
        return json.loads(result.stdout)["ok"] if result.returncode == 0 else {}

    def describe(self):
        """The screens, the caret, the listing, the host's log and the fake model's requests, for
        failure messages."""
        parts = []
        diagnostic = getattr(self, 'resume_diagnostic', None)
        if diagnostic is not None:
            parts.append('original Codex resume evidence:\n' + diagnostic.finish(self.agent_home))
        for name in self.names:
            try:
                parts.append(f"screen of {name}:\n{self.capture(name)}")
                pane = self.pane(name)
                parts.append(f"pane of {name}: " + json.dumps(
                    {key: pane.get(key) for key in ("width", "height", "cursor_x", "cursor_y", "dead")}))
                styled = self.hgs("__host", "call", json.dumps(
                    {"op": "capture", "target": {"name": name}, "styled": True}), rc=None)
                rows = (json.loads(styled.stdout)["ok"]["text"].splitlines()
                        if styled.returncode == 0 else [])
                cursor_y = pane.get("cursor_y")
                if isinstance(cursor_y, int) and 0 <= cursor_y < len(rows):
                    parts.append(f"styled row {cursor_y} of {name}: {rows[cursor_y]!r}")
                parts.append(f"record of {name}: " + json.dumps(
                    {key: self.record(name).get(key) for key in ("activity", "phase", "conversation_id", "run_id",
                                                               "expected_id", "error", "input_pending_at",
                                                               "last_event_at")}))
            except Exception as error:  # noqa: BLE001 (diagnostics never hide the failure)
                parts.append(f"{name}: {error}")
        try:
            log = (self.state / "host.log").read_text(encoding="utf-8", errors="replace").splitlines()
            parts.append("host.log:\n" + "\n".join(log[-20:]))
        except OSError:
            pass
        try:
            requests = [{key: request.get(key) for key in ("route", "prompt", "answer", "results")}
                        for request in api.requests(self.api_root)[-8:]]
            parts.append("model requests:\n" + "\n".join(json.dumps(request, ensure_ascii=False)[:600]
                                                          for request in requests))
        except (OSError, ValueError) as error:
            parts.append(f"model requests: {error}")
        return "\n" + "\n".join(parts)

    def until(self, what, predicate, timeout=WAIT):
        deadline = time.monotonic() + timeout
        while True:
            value = predicate()
            if value:
                return value
            if time.monotonic() > deadline:
                self.fail(f"timed out after {timeout} s waiting for {what}" + self.describe())
            time.sleep(0.25)

    # Processes.

    def pin_session(self, name):
        """Keep handles on the agent and supervisor of a session while they run."""
        record = self.record(name)
        supervisor = record.get("supervisor") if isinstance(record.get("supervisor"), dict) else {}
        for pid, started in ((record.get("pid"), record.get("process_start")),
                             (supervisor.get("pid"), supervisor.get("start"))):
            if isinstance(pid, int) and isinstance(started, str) and started and (pid, started) not in self.pinned:
                handle = host.open_host(pid, started)
                if handle:
                    self.pinned[pid, started] = handle

    def alive(self, pid, started):
        """Whether this exact process still runs: its creation time matches and it has not exited."""
        handle = host.open_host(pid, started)
        if not handle:
            return False
        try:
            return host.KERNEL32.WaitForSingleObject(handle, 0) != host.WAIT_OBJECT_0
        finally:
            host.KERNEL32.CloseHandle(handle)

    def stop_team_worker(self):
        """Task #52 (team ede724c4): a case's `hgs team` command starts the team worker from the
        case's portable hgs.exe, and the worker runs until no team runs. It is no session and no
        host process, so nothing else stopped it, and it held portable\hgs.exe when the case's
        folder went (CI #21: PermissionError [WinError 5] after the U54 team swap). It is asked to
        end in the case's own state; a case that started no worker hears "no team worker runs"."""
        if getattr(self, "hgs_exe", None) and self.hgs_exe.is_file():
            self.hgs("team", "worker", "--stop", rc=None)

    def stop_host(self):
        """Kill the sessions and stop the host (HostCase), then wait for every agent and
        supervisor of the case, and for its executables to be released, before its
        folders go; end and report any process that stays."""
        try:
            self.stop_team_worker()
            for name in self.names:
                self.pin_session(name)
            super().stop_host()
        finally:
            left = []
            deadline = time.monotonic() + WAIT
            for (pid, _), handle in self.pinned.items():
                remaining = max(0, int((deadline - time.monotonic()) * 1000))
                if host.KERNEL32.WaitForSingleObject(handle, remaining) != host.WAIT_OBJECT_0:
                    left.append(pid)
                    host.KERNEL32.TerminateProcess(handle, 1)
                    host.KERNEL32.WaitForSingleObject(handle, WAIT * 1000)
                host.KERNEL32.CloseHandle(handle)
            self.pinned.clear()
            # The host's OpenConsole.exe ends after its pseudoconsoles, and a scanner may
            # hold a fresh copy for a moment (tests/test_windows_host_conpty.py): wait
            # until no process holds an executable or library of the case.
            files = [path for pattern in ("*.exe", "*.dll") for path in self.root.rglob(pattern)]
            deadline = time.monotonic() + host.LIMIT
            while time.monotonic() < deadline and any(map(in_use, files)):
                time.sleep(0.25)
            if self.project_parent:
                shutil.rmtree(self.project_parent, ignore_errors=True)
        if left:
            raise AssertionError(f"agent processes survived the session host: {sorted(left)}")

    # Sessions.

    def launch(self, tag, *extra):
        """A tracked session of the agent in the project, started as the GUI starts one; its name."""
        name = f"{self.AGENT}/project/{tag}"
        launch_id = str(uuid.uuid4())
        args = [*self.ARGS, *extra]
        result = self.hgs(self.AGENT, str(self.project), "--new", "-n", tag, "-d", "--launch-id", launch_id,
                          *(["--", *args] if args else []))
        self.assertEqual(result.stdout, f"hgs: started {name}\n", result.stderr)
        self.names.append(name)
        status = self.status()  # Also remembers the host process for the cleanup.
        if CONPTY and status.get("running"):
            # The staged image carries the pair, as the zip's does (spec D6).
            self.assertEqual(status["status"]["conpty"], "bundled", status)
        row = self.until(f"{name} in the listing", lambda: self.row(name).get("runtime_state") == "live"
                         and self.row(name))
        self.assertEqual((row["cmd"], row["project"], row["tag"], row["launch_id"], row["process_state"]),
                         (self.AGENT, "project", tag, launch_id, "running"))
        self.pin_session(name)
        return name

    def settled(self, name):
        """The screen once it shows the agent's composer and stays the same for half a second."""
        def same():
            first = self.capture(name)
            time.sleep(0.5)
            second = self.capture(name)
            composer = any(line.strip().startswith(self.COMPOSER) for line in second.splitlines())
            return second if first == second and composer else None
        return self.until(f"{name}'s composer", same)

    def sample_resume(self):
        diagnostic = getattr(self, 'resume_diagnostic', None)
        if diagnostic is None:
            return
        def query(request):
            try:
                result = subprocess.run([str(self.hgs_exe), '__host', 'call', json.dumps(request)],
                    env=self.env, cwd=self.work, stdin=subprocess.DEVNULL, capture_output=True,
                    timeout=2, creationflags=host.HIDDEN)
                if result.returncode == 0 and len(result.stdout) <= 64 * 1024:
                    return json.loads(result.stdout).get('ok')
            except (OSError, ValueError, subprocess.TimeoutExpired):
                pass
            return None
        try:
            diagnostic.sample(self.record(diagnostic.name), query)
        except (OSError, ValueError):
            pass  # Diagnostics never replace the original test assertion.

    def ready(self, name):
        """The listing row once the session takes a message: what hgs itself reports for a
        conversation the agent has yet to confirm, else idle with a settled composer."""
        def check():
            self.sample_resume()
            row, state = self.row(name), self.inspect(name)
            if row.get("runtime_state") != "live" or state.get("pending_questions"):
                return None
            if not row.get("conversation_id"):
                return row if state.get("first_message_can_send") else None
            if "resume_message_can_send" in state:
                return row if state["resume_message_can_send"] else None
            return row if row.get("activity") == "idle" and row.get("phase") == "idle" else None
        row = self.until(f"{name} to take a message", check)
        self.settled(name)
        return self.row(name) or row

    def message(self, row, text):
        return dict(request_id=str(uuid.uuid4()), text=text, attachments=[], expected_run_id=row["run_id"],
                    expected_conversation_id=row.get("conversation_id") or "")

    def send(self, name, payload):
        return json.loads(self.hgs("send", name, "--json", input=json.dumps(payload)).stdout)

    def turn_ended(self, name, since):
        """inspect once a Stop hook later than `since` has left the session idle."""
        state = self.inspect(name)
        stops = [event for event in state.get("events", []) if event.get("type") == "Stop"
                 and event.get("at", 0) >= since]
        return state if stops and state.get("activity") == "idle" and state.get("phase") == "idle" else None

    def deliver(self, name, text):
        """Send `text` once the session is ready and wait for its turn to end; the request and receipt."""
        row = self.ready(name)
        payload = self.message(row, text)
        receipt = self.send(name, payload)
        self.assertEqual((receipt["status"], receipt["submitted_text"], receipt["run_id"]),
                         ("submitted", text, row["run_id"]), receipt)
        self.until(f"the turn of {text!r}", lambda: self.turn_ended(name, receipt["submitted_at"]))
        self.until(f"{text!r} in the transcript", lambda: self.accepted(name, text))
        return payload, receipt

    def card(self, name, source):
        """The pending card of `source` once hgs can answer it."""
        seen = {}

        def answerable():
            cards = [card for card in self.inspect(name).get("pending_questions", []) if card.get("source") == source]
            if cards:
                seen["card"] = cards[0]
            return cards[0] if cards and cards[0].get("can_answer") else None
        try:
            return self.until(f"an answerable {source} card", answerable)
        except AssertionError as error:
            raise AssertionError(f"{error}\nlast card: {json.dumps(seen.get('card'), ensure_ascii=False)}") from None

    def answer(self, name, card, answers, statuses=("answered",)):
        """Answer a card through `hgs answer`; the request and its receipt, whose status is one
        of `statuses`."""
        row = self.row(name)
        payload = dict(request_id=str(uuid.uuid4()), expected_run_id=row["run_id"],
                       expected_conversation_id=row.get("conversation_id") or "", question_id=card["question_id"],
                       expected_question_hash=card["question_hash"], answers=answers)
        receipt = self.json("answer", name, "--json", input=json.dumps(payload))
        self.assertIn(receipt["status"], statuses, receipt)
        return payload, receipt

    def check_clear(self, name, status):
        """`hgs clear-context` once the session is idle; the row and record before it and the
        receipt, whose status is `status` (either outcome for None)."""
        row, record = self.ready(name), self.record(name)
        request = dict(request_id=str(uuid.uuid4()), expected_run_id=row["run_id"],
                       expected_conversation_id=row["conversation_id"])
        receipt = self.json("clear-context", name, "--json", input=json.dumps(request))
        self.assertEqual((receipt["run_id"], receipt["conversation_id"]), (row["run_id"], row["conversation_id"]))
        self.assertIn(receipt["status"], (status,) if status else ("confirmed", "submitted"), receipt)
        return row, record, receipt

    def check_after_clear(self, name, row, record):
        """The same agent process takes the next message in a new conversation; the old
        conversation keeps its own messages."""
        text = "After the clear."
        self.deliver(name, text)
        cleared, current = self.row(name), self.record(name)
        self.assertNotIn(cleared["conversation_id"], ("", row["conversation_id"]))
        self.assertEqual((cleared["run_id"], current["pid"], current["process_start"]),
                         (row["run_id"], record["pid"], record["process_start"]))
        self.assertEqual(self.accepted(name, text, cleared["conversation_id"]), 1, self.describe())
        self.assertEqual(self.accepted(name, text, row["conversation_id"]), 0)
        self.assertEqual(self.accepted(name, MESSAGES[0], row["conversation_id"]), 1)
        self.assertIn(row["conversation_id"], self.inspect(name)["cleared_conversations"])

    def model_results(self):
        """The tool results the fake model has received, in order."""
        return [result for request in api.requests(self.api_root) for result in request.get("results") or []]

    # The native transcript.

    def transcript_rows(self, conversation_id):
        """The rows of the agent's own transcript of one conversation."""
        raise NotImplementedError

    def accepted(self, name, text, conversation_id=None):
        """How many user messages of exactly `text` the agent recorded in a conversation."""
        raise NotImplementedError

    # Cases shared by both agents.

    def check_messages(self, name):
        """Each message of MESSAGES starts exactly one turn; a replayed request delivers nothing."""
        for text in MESSAGES:
            payload, receipt = self.deliver(name, text)
            self.assertEqual(self.accepted(name, text), 1, self.describe())
            if text == MESSAGES[0]:
                self.assertEqual(self.send(name, payload), receipt)
        # Every turn has ended; the replay typed nothing that a later turn could hold.
        self.settled(name)
        self.assertEqual([self.accepted(name, text) for text in MESSAGES], [1] * len(MESSAGES), self.describe())

    def keys(self, name, *keys):
        """Keys to the session's pane through `hgs __host backend` op `send`, the path hgs uses."""
        panes = json.loads(self.hgs("__host", "call", json.dumps({"op": "list"})).stdout)["ok"]["panes"]
        pane = next(pane["pane"] for pane in panes if pane["session"] == name)
        self.hgs("__host", "backend", json.dumps({"op": "send", "pane": pane, "keys": list(keys)}))

    def composer(self, name):
        """The text after the agent's composer marker on its last composer row, or None."""
        rows = [line.strip() for line in self.capture(name).splitlines() if line.strip().startswith(self.COMPOSER)]
        return rows[-1][len(self.COMPOSER):].strip() if rows else None

    def check_keys(self, name):
        """Spike S2 through the host (zerus-ziq3.7.13): text typed after a lone Esc arrives whole,
        Ctrl+C clears a draft, Ctrl+S neither freezes the screen (XOFF) nor stops input, and the
        keys hgs itself clears Claude's input with (C-e C-u: claude_question.rs, interrupt.rs;
        hgs never clears Codex's composer with keys) empty it. Each step is checked on the screen,
        and the empty composer by an exact `hgs send`, which refuses a non-empty one. Codex
        resolves Ctrl+letter in the keyboard layout (zerus-c8r9): a non-Latin layout reproduces
        what a hosted runner's US layout cannot."""
        self.ready(name)
        pid = self.record(name).get("pid")
        shows = lambda text: lambda: text in (self.composer(name) or "")  # noqa: E731
        cleared = lambda text: lambda: (lambda row: row is not None and text not in row)(self.composer(name))  # noqa: E731
        # A lone Esc, then text: nothing of it is lost or taken as Alt+key, and nothing is cancelled.
        self.keys(name, {"named": "Escape"})
        time.sleep(0.5)
        self.keys(name, {"literal": "esc probe"})
        self.until("the text typed after a lone Esc", shows("esc probe"))
        # Ctrl+C clears a draft (one press on a draft: a second one on an empty composer quits).
        self.keys(name, {"named": "C-c"})
        self.until("Ctrl+C to clear the draft", cleared("esc probe"))
        if self.CLEARS_WITH_KEYS:
            self.keys(name, {"literal": "clear probe"})
            self.until("the draft before C-e C-u", shows("clear probe"))
            self.keys(name, {"named": "C-e"}, {"named": "C-u"})
            self.until("C-e C-u to empty the composer", cleared("clear probe"))
        # Ctrl+S on the empty composer (Claude stashes a draft with it): what follows still shows.
        time.sleep(2)  # Past the agents' "Ctrl+C again to quit" window.
        self.keys(name, {"named": "C-s"})
        self.keys(name, {"literal": "ctrl s probe"})
        self.until("text typed after Ctrl+S", shows("ctrl s probe"))
        self.keys(name, {"named": "C-c"})
        self.until("Ctrl+C to clear the draft", cleared("ctrl s probe"))
        # The same agent process takes the next message exactly once; no probe became one.
        self.deliver(name, MESSAGES[0])
        self.assertEqual(self.record(name).get("pid"), pid, self.describe())
        self.assertEqual([self.accepted(name, text) for text in (MESSAGES[0], "esc probe", "clear probe",
                                                                 "ctrl s probe")],
                         [1, 0, 0, 0], self.describe())

    def check_lifecycle(self, name):
        """Pause ends the agent, resume continues the same conversation, terminate archives it."""
        self.deliver(name, MESSAGES[0])
        row, record = self.row(name), self.record(name)
        process = (record["pid"], record["process_start"])
        self.assertEqual(self.hgs("pause", name).stdout, f"hgs: paused {name}\n")
        paused = self.until("the paused session", lambda: (lambda row: row.get("state") == "paused" and row)(
            self.row(name)))
        self.assertEqual({key: paused.get(key) for key in ("runtime_state", "process_state", "resumable",
                                                           "conversation_id")},
                         dict(runtime_state="stopped", process_state="exited", resumable=True,
                              conversation_id=row["conversation_id"]))
        self.until("the paused agent to exit", lambda: not self.alive(*process))
        if self.AGENT == 'codex':
            self.resume_diagnostic = ResumeDiagnostics(name, record)
        self.hgs("resume", name, "-d")
        self.sample_resume()
        self.until("the resumed session", lambda: (lambda row: row.get("runtime_state") == "live"
                                                   and row.get("run_id") != record["run_id"])(self.row(name)))
        self.pin_session(name)
        # Codex confirms a resumed conversation only with its next message.
        text = "After the resume."
        self.deliver(name, text)
        # Once it has answered, the record names the agent rather than its supervisor.
        self.pin_session(name)
        resumed = self.record(name)
        self.assertNotEqual((resumed.get("pid"), resumed.get("process_start")), process)
        self.assertEqual(self.row(name)["conversation_id"], row["conversation_id"])
        self.assertEqual(self.accepted(name, text, row["conversation_id"]), 1, self.describe())
        self.assertEqual(self.accepted(name, MESSAGES[0], row["conversation_id"]), 1, self.describe())
        process = (resumed["pid"], resumed["process_start"])
        result = self.hgs("terminate", name)
        self.assertTrue(result.stdout.startswith(f"hgs: terminated and archived {name} ("), result.stdout)
        archived = self.row(name)
        self.assertEqual({key: archived.get(key) for key in ("state", "runtime_state", "conversation_id")},
                         dict(state="archived", runtime_state="stopped", conversation_id=row["conversation_id"]))
        self.until("the terminated agent to exit", lambda: not self.alive(*process))

    # Changing the account of a running session (U53).

    def second_account(self):
        """A second account of this agent, signed in on this computer, and its folder."""
        self.hgs("account", "add", "second", "--provider", self.AGENT, "--label", "Second", "--json")
        folder = self.home / ".config" / "hgs" / "accounts" / "second"
        self.assertTrue(folder.is_dir(), "the account profile folder is missing")
        self.prepare_account(folder)
        self.second_home = folder
        return folder

    def check_switch_account(self, tag):
        """`hgs switch-account` moves the running session to another account of the same
        agent: the same conversation, folder and name, copied into the new account's native
        store, and the agent resumes it there; the old account keeps its own copy."""
        name = self.launch(tag, *([self.NO_HOOK_REVIEW] if self.NO_HOOK_REVIEW else []))
        # A refused send names which empty-composer check refused and where the
        # caret is (Claude 2.1.312 parks it outside the prompt row).
        self.env["HGS_INPUT_TRACE"] = "1"
        # A real turn first: Claude writes its transcript only after the first prompt, and
        # Codex names it in the record only then.
        self.deliver(name, MESSAGES[0])
        row = self.ready(name)
        conversation = row["conversation_id"]
        before = self.record(name)
        transcript = Path(before["transcript"])
        self.assertTrue(transcript.is_file(), before)
        original = transcript.read_bytes()
        folder = self.second_account()
        receipt = self.json("switch-account", name, "--account", "second",
                            "--request-id", str(uuid.uuid4()))
        self.assertEqual(receipt["status"], "switched", receipt)
        self.assertEqual((receipt["conversation_id"], receipt["from"]["account_id"],
                          receipt["to"]["account_id"]), (conversation, "native-" + self.AGENT, "second"))
        # The same conversation, folder and name, on the new account.
        switched = self.record(name)
        self.assertEqual((switched["name"], switched["launch_dir"], switched["conversation_id"],
                          switched["account_id"], switched["agent_home"]),
                         (name, str(self.project), conversation, "second", str(folder)))
        self.assertNotEqual(switched["run_id"], before["run_id"])
        # A resumed Codex is a deferred resume until its first turn: its own
        # supervisor keeps the expected id equal to the conversation. Claude's
        # SessionStart confirms the resume and clears it. Neither may name
        # another conversation.
        expected = switched.get("expected_id")
        if self.AGENT == "codex":
            self.assertIn(expected, (None, "", conversation), expected)
        else:
            self.assertFalse(expected, expected)
        # The copy is in the new account's store and starts with the conversation's
        # records: the resumed agent may append to it. The old account's file is
        # final only after its agent stops: it may grow past the snapshot taken
        # before the switch (Claude appends its cost-state record while stopping),
        # and the copy holds those final bytes.
        copy = next(folder.rglob(f"*{conversation}.jsonl"))
        copied = copy.read_bytes()
        old_final = transcript.read_bytes()
        self.assertTrue(old_final.startswith(original), "the old account's records were rewritten")
        self.assertTrue(copied.startswith(old_final), "the copy does not hold the old file's final records")
        self.assertTrue(transcript.is_file())
        # The listing and the inspection both name the account.
        self.assertEqual((self.row(name)["account_id"], self.row(name)["account_label"]),
                         ("second", "Second"))
        inspected = self.inspect(name)
        self.assertEqual((inspected["account_id"], inspected["account_label"]), ("second", "Second"))
        # The resumed agent answers a second prompt in that exact conversation.
        self.pin_session(name)
        text = "After the account switch."
        self.deliver(name, text)
        self.assertEqual(self.row(name)["conversation_id"], conversation)
        self.assertEqual(self.accepted(name, text, conversation), 1, self.describe())
        self.assertEqual(self.accepted(name, MESSAGES[0], conversation), 1, self.describe())
        self.until(f"{text!r} at the model", lambda: any(
            text in json.dumps(request) for request in api.requests(self.api_root)))


@unittest.skipUnless(CLAUDE and HGS_SET, "set HGS_TEST_BIN and HGS_CLAUDE_TEST_BIN to run real Claude Code")
class NativeWindowsClaude(NativeAgent, host.HostCase):
    AGENT = "claude"
    BINARY = CLAUDE
    ARGS = ("--model", "claude-sonnet-4-6", "--permission-mode", "manual")
    COMPOSER = "❯"
    CLEARS_WITH_KEYS = True

    def configure(self):
        projects = {form: {"hasTrustDialogAccepted": True, "allowedTools": []} for form in path_forms(self.project)}
        (self.agent_home / ".claude.json").write_text(json.dumps({
            "hasCompletedOnboarding": True, "theme": "dark", "autoUpdates": False,
            "customApiKeyResponses": {"approved": [KEY[-20:]], "rejected": []},
            "officialMarketplaceAutoInstallAttempted": True, "hasSeenAutoDefaultNudge": True,
            "projects": projects}), encoding="utf-8")
        (self.agent_home / "settings.json").write_text(json.dumps({"permissions": {"defaultMode": "default"}}),
                                                       encoding="utf-8")
        self.env["CLAUDE_CONFIG_DIR"] = str(self.agent_home)

    def prepare_account(self, folder):
        """A managed account's session blanks ANTHROPIC_* (accounts::select); its own
        settings carry the fixture's base URL and key, as a signed-in account's
        preferences carry its own configuration."""
        projects = {form: {"hasTrustDialogAccepted": True, "allowedTools": []} for form in path_forms(self.project)}
        (folder / ".claude.json").write_text(json.dumps({
            "hasCompletedOnboarding": True, "theme": "dark", "autoUpdates": False,
            "customApiKeyResponses": {"approved": [KEY[-20:]], "rejected": []},
            "officialMarketplaceAutoInstallAttempted": True, "hasSeenAutoDefaultNudge": True,
            "projects": projects}), encoding="utf-8")
        (folder / "settings.json").write_text(json.dumps({
            "permissions": {"defaultMode": "default"}, "theme": "dark",
            "env": {"ANTHROPIC_BASE_URL": self.api_url, "ANTHROPIC_API_KEY": KEY,
                    "CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC": "1"}}), encoding="utf-8")

    def transcript_rows(self, conversation_id):
        # After a switch the conversation lives in the new account's store; the
        # old account keeps only its own copy of the same rows.
        homes = ([self.second_home] if self.second_home else []) + [self.agent_home]
        paths = []
        for home in homes:
            paths = list((home / "projects").glob(f"*/{conversation_id}.jsonl"))
            if paths:
                break
        rows = []
        for path in paths:
            for line in path.read_text(encoding="utf-8").splitlines():
                try:
                    rows.append(json.loads(line))
                except ValueError:
                    pass  # A line still being written.
        return rows

    def check_hook_form(self):
        """Claude Code from 2.1.296 gets the exec form naming the stable CLI (spec D12), and the
        answer channel beside its PermissionRequest hook in the same form."""
        settings = json.loads((self.agent_home / "settings.json").read_text(encoding="utf-8"))
        stable = os.path.realpath(self.bin / "hgs.exe").replace(os.sep, "/")
        hook = {"hooks": [{"type": "command", "command": stable, "args": ["__state", "hook"], "timeout": 5}]}
        self.assertEqual(settings["hooks"]["Stop"], [hook])
        self.assertEqual(settings["hooks"]["PermissionRequest"], [hook, {"hooks": [{
            "type": "command", "command": stable, "args": ["__state", "answer-channel"], "timeout": 86400}]}])

    def accepted(self, name, text, conversation_id=None):
        conversation_id = conversation_id or self.row(name).get("conversation_id")
        forms = (text, [{"type": "text", "text": text}])
        return sum(row.get("type") == "user" and row.get("message", {}).get("content") in forms
                   for row in self.transcript_rows(conversation_id))

    def test_project_powershell_wrapper_keeps_existing_tracking_and_exact_messages(self):
        # Initialize this private profile through the native CLI once. A project
        # script is deliberately not run as a version probe for fresh hooks.
        prime = self.launch("prime-hooks")
        self.until("the native conversation", lambda: self.row(prime).get("conversation_id"))
        self.hgs("terminate", prime)
        self.until("the prime archive", lambda: self.row(prime).get("state") == "archived")
        settings = (self.agent_home / "settings.json").read_bytes()
        program = str(self.bin / "claude.exe").replace("'", "''")
        (self.project / "run_claude.ps1").write_text(f"& '{program}' @args; exit $LASTEXITCODE\n", encoding="utf-8-sig")
        name = self.launch("wrapper")
        self.until("the wrapper's native conversation", lambda: self.row(name).get("conversation_id"))
        self.assertTrue(self.inspect(name)["tracked"])
        self.assertEqual((self.agent_home / "settings.json").read_bytes(), settings)
        self.check_messages(name)

    def test_a_tracked_launch_reports_its_hooks_and_send_delivers_exact_messages(self):
        name = self.launch("messages")
        # Claude's SessionStart hook confirms the conversation before any message.
        row = self.until("the confirmed conversation", lambda: (lambda row: row.get("conversation_id")
                                                                and row.get("activity") == "idle" and row)(
            self.row(name)))
        self.assertTrue(self.inspect(name)["tracked"])
        self.check_hook_form()
        self.check_messages(name)
        # The hook named the transcript that Claude has written since.
        self.assertTrue(os.path.samefile(self.record(name)["transcript"], next(
            (self.agent_home / "projects").glob(f"*/{row['conversation_id']}.jsonl"))))
        events = [event["type"] for event in self.inspect(name)["events"]]
        self.assertTrue({"SessionStart", "UserPromptSubmit", "Stop"} <= set(events), events)
        journal = self.journal(row["run_id"])
        self.assertEqual(journal.count("UserPromptSubmit"), len(MESSAGES), journal)

    def test_hgs_answers_ask_user_question(self):
        name = self.launch("question")
        self.deliver_prompt(name, api.QUESTIONS)
        card = self.channel_card(name, "hook")
        self.assertEqual([item["question"] for item in card["questions"]],
                         [item["question"] for item in api.QUESTION_ITEMS])
        answers = [dict(question_id="q_0", selected_option_ids=["opt_0_1"], text=""),
                   dict(question_id="q_1", selected_option_ids=["opt_1_0", "opt_1_2"], text=""),
                   dict(question_id="q_2", selected_option_ids=[], text="Привет from Zerus")]
        payload, receipt = self.answer(name, card, answers)
        # The answer channel took it, so no key reached Claude's panel.
        self.assertEqual(receipt["transport"], "claude_hook", receipt)
        self.assertEqual(len(self.channels("emitted.json")), 1)
        # The same request is acknowledged again without a second delivery.
        self.assertEqual(self.json("answer", name, "--json", input=json.dumps(payload)), receipt)
        expected = {"Which release should we prepare?": "Preview", "Which checks should we run?": "Unit, UI",
                    "What else should we know?": "Привет from Zerus"}

        def recorded():
            for row in self.transcript_rows(card["conversation_id"]):
                result = row.get("toolUseResult")
                if isinstance(result, dict) and "answers" in result:
                    return result
            return None
        result = self.until("the answers in the transcript", recorded)
        self.assertEqual(result["answers"], expected)
        self.until("the answers at the model", lambda: any(
            all(f'"{question}"="{answer}"' in str(item) for question, answer in expected.items())
            for item in self.model_results()))
        state = self.until("the end of the turn", lambda: self.turn_ended(name, receipt["submitted_at"]))
        self.assertEqual(state["pending_questions"], [])
        self.assertEqual([event["type"] for event in state["events"]].count("QuestionAnswered"), 1)

    def test_hgs_answers_a_question_taller_than_its_unviewed_pane(self):
        # Pane growth is the behavior under test, so this session has no answer
        # channel: its hooks stay in the shell form an earlier hgs wrote, which
        # hgs repeats for every event and never pairs with a channel. Claude
        # runs that form with Git Bash, found as Claude Code 2.1.296 finds it.
        git = shutil.which("git", path=self.env.get("PATH") or self.env.get("Path"))
        bashes = [Path(self.env.get("CLAUDE_CODE_GIT_BASH_PATH", "")), Path(r"C:\Program Files\Git\bin\bash.exe"),
                  Path(r"C:\Program Files (x86)\Git\bin\bash.exe"), *([Path(git).parent.parent / "bin" / "bash.exe"] if git else [])]
        self.assertTrue(any(bash.name and bash.is_file() for bash in bashes),
                        "the shell-form hooks of this case need Git for Windows")
        stable = os.path.realpath(self.bin / "hgs.exe").replace(os.sep, "/")
        settings = json.loads((self.agent_home / "settings.json").read_text(encoding="utf-8"))
        settings["hooks"] = {"PermissionRequest": [{"hooks": [
            {"type": "command", "command": f'"{stable}" __state hook', "timeout": 5}]}]}
        (self.agent_home / "settings.json").write_text(json.dumps(settings), encoding="utf-8")
        name = self.launch("tall")
        written = json.loads((self.agent_home / "settings.json").read_text(encoding="utf-8"))["hooks"]
        self.assertEqual(written["PermissionRequest"], settings["hooks"]["PermissionRequest"])
        self.assertNotIn("answer-channel", json.dumps(written))
        self.deliver_prompt(name, api.TALL_QUESTION)
        # No client views the pane, so the session host grows it from 80x24
        # until Claude redraws the whole panel; the screen is the only path here.
        card = self.card(name, "hook")
        self.assertEqual(card["answer_transport"], "claude_tui")
        details = json.loads(self.hgs("__host", "call", json.dumps(
            {"op": "pane_info", "target": {"name": name}})).stdout)["ok"]
        self.assertEqual(details["width"], 80)
        self.assertGreater(details["height"], 24)
        payload, receipt = self.answer(name, card, [dict(question_id="q_0",
                                                         selected_option_ids=["opt_0_0", "opt_0_2"], text="")])
        expected = {"Which release should we prepare?": "API-ключи, Свои адреса"}

        def recorded():
            for row in self.transcript_rows(card["conversation_id"]):
                result = row.get("toolUseResult")
                if isinstance(result, dict) and "answers" in result:
                    return result
            return None
        self.assertEqual(self.until("the answer in the transcript", recorded)["answers"], expected)

    def test_hgs_approves_a_write_once(self):
        name = self.launch("approval")
        self.deliver_prompt(name, api.WRITE)
        card = self.card(name, "claude_tool_approval")
        self.assertTrue(card["approval"])
        options = card["questions"][0]["options"]
        self.assertEqual((options[0], options[-1]["label"], options[-1]["scope"]),
                         (dict(id="1", label="Yes", scope="once"), "No", "once"))
        self.assertIn(api.APPROVED_FILE, card["questions"][0]["question"])
        approved = self.project / api.APPROVED_FILE
        self.assertFalse(approved.exists())
        _, receipt = self.answer(name, card, [dict(question_id="approval", selected_option_ids=["1"], text="")])
        self.until("the approved file", approved.is_file)
        self.assertEqual(approved.read_text(encoding="utf-8"), api.APPROVED_TEXT)
        state = self.until("the end of the turn", lambda: self.turn_ended(name, receipt["submitted_at"]))
        self.assertEqual(state["pending_questions"], [])
        # A one-time approval leaves the permission mode as it was.
        settings = json.loads((self.agent_home / "settings.json").read_text(encoding="utf-8"))
        self.assertEqual(settings["permissions"], {"defaultMode": "default"})

    def test_the_answer_channel_answers_bash_approvals(self):
        name = self.launch("channel")
        self.check_hook_form()
        approved = self.project / api.BASH_FILE
        # Yes: the command runs once and the permission mode stays as it was.
        card = self.hook_card(name, api.BASH)
        self.assertTrue(card["panel_verified"], card)
        self.assertEqual(card["questions"][0]["options"][0], dict(id="1", label="Yes", scope="once"))
        payload, receipt = self.answer(name, card, self.choose(card, "Yes"))
        self.assertEqual(receipt["transport"], "claude_hook", receipt)
        self.until("the approved file", approved.is_file)
        self.until("the end of the turn", lambda: self.turn_ended(name, receipt["submitted_at"]))
        self.assertEqual(self.json("answer", name, "--json", input=json.dumps(payload)), receipt)
        self.assertEqual(json.loads((self.agent_home / "settings.json").read_text(encoding="utf-8"))["permissions"],
                         {"defaultMode": "default"})
        approved.unlink()
        # Feedback: Claude gets the text instead of running the command and goes on.
        card = self.hook_card(name, api.BASH)
        text = "Use printf other instead"
        _, receipt = self.answer(name, card, [dict(question_id="approval", selected_option_ids=[], text=text)])
        self.assertEqual(receipt["transport"], "claude_hook", receipt)
        self.until("the feedback at the model", lambda: any(text in json.dumps(result, ensure_ascii=False)
                                                            for result in self.model_results()))
        self.until("the end of the turn", lambda: self.turn_ended(name, receipt["submitted_at"]))
        # No: Claude stops the turn as its own No does.
        card = self.hook_card(name, api.BASH)
        _, receipt = self.answer(name, card, self.choose(card, "No"))
        self.assertEqual(receipt["transport"], "claude_hook", receipt)
        self.until("Claude's interruption", lambda: "What should Claude do instead?" in self.capture(name))
        state = self.until("the interrupted turn", lambda: (lambda state: state if state.get("phase") == "interrupted"
                                                            and not state.get("pending_questions") else None)(
            self.inspect(name)))
        self.assertFalse(approved.exists(), state)
        self.assertEqual(len(self.channels("emitted.json")), 3)

    def test_a_bash_panel_taller_than_80x24_is_answered_through_the_hook(self):
        # The host's size for a session no client views (host::SIZE): the panel loses its top,
        # and keeps losing it at the most hgs grows such a pane.
        name = self.launch("clipped")
        card = self.hook_card(name, api.LONG_BASH)
        question = card["questions"][0]
        self.assertFalse(card["panel_verified"], card)
        self.assertEqual([(option["label"], option["scope"]) for option in question["options"]],
                         [("Yes", "once"), ("No", "once")])
        self.assertIn(api.BASH_INPUT["command"], question["body"])
        _, receipt = self.answer(name, card, self.choose(card, "Yes"))
        self.assertEqual(receipt["transport"], "claude_hook", receipt)
        self.until("the approved file", (self.project / api.BASH_FILE).is_file)
        self.until("the end of the turn", lambda: self.turn_ended(name, receipt["submitted_at"]))

    def test_terminal_answers_end_the_hook(self):
        name = self.launch("terminal")
        approved = self.project / api.BASH_FILE
        # Claude tells the hook nothing of a Terminal Yes: the run's end closes it.
        for key, check in (("Enter", approved.is_file),
                           ("Escape", lambda: "What should Claude do instead?" in self.capture(name))):
            card = self.hook_card(name, api.BASH)
            def current():
                return next((item for item in self.inspect(name).get('pending_questions', [])
                             if item.get('question_id') == card['question_id']), None)
            card = self.until('the original complete Bash permission panel',
                              lambda: (item := current()) and item.get('panel_verified') and item)
            carded = time.monotonic()
            worker = self.worker()
            wait_claude_permission_guard(card, current, carded)
            self.keys(name, {"named": key})
            # zerus-dy5u: an intermittent Enter without effect; the failure
            # must tell a key sent before the panel from one that was lost.
            sent = (f"panel_verified=True, native_guard={CLAUDE_PERMISSION_INPUT_GUARD:.3f} s, "
                    f"sent {time.monotonic() - carded:.3f} s after the complete card")
            self.until(f"Terminal's {key} ({sent})", check)
            self.until(f"the hook to end after {key}", lambda: not self.alive(*worker))
            self.until("no pending card", lambda: not self.inspect(name).get("pending_questions"))
            self.refused(name, card, self.choose(card, "No"))
            self.assertEqual(self.channels("answer.json") + self.channels("claimed.json")
                             + self.channels("emitted.json"), [])
            if key == "Enter":
                self.until("the end of the turn", lambda: self.turn_ended(name, 0))
        self.assertFalse(self.channels("retracted.json"))

    def test_a_killed_hook_leaves_the_approval_to_the_verified_screen(self):
        name = self.launch("killed")
        self.hook_card(name, api.BASH)
        handle = host.open_host(*self.worker())
        self.assertTrue(handle)
        try:
            self.assertTrue(host.KERNEL32.TerminateProcess(handle, 1), "the owned hook did not terminate")
            self.assertEqual(host.KERNEL32.WaitForSingleObject(handle, WAIT * 1000), 0,
                             "the owned hook's handle did not signal its exit")
        finally:
            host.KERNEL32.CloseHandle(handle)
        card = self.until("the screen card", lambda: next((card for card in self.inspect(name)["pending_questions"]
                                                           if card.get("answer_transport") == "claude_tui"
                                                           and card.get("can_answer")), None))
        _, receipt = self.answer(name, card, self.choose(card, "Yes"))
        self.assertEqual(receipt["transport"], "claude_tui", receipt)
        self.until("the approved file", (self.project / api.BASH_FILE).is_file)
        self.assertEqual(self.channels("answer.json") + self.channels("claimed.json"), [])

    def test_clear_context_starts_a_new_conversation_in_the_same_agent(self):
        name = self.launch("clear")
        self.deliver(name, MESSAGES[0])
        # Claude's SessionStart hook reports the new conversation before the receipt.
        row, record, _ = self.check_clear(name, "confirmed")
        self.assertNotIn(self.row(name)["conversation_id"], ("", row["conversation_id"]))
        self.assertEqual(self.inspect(name)["session_clear"]["type"], "SessionCleared")
        self.check_after_clear(name, row, record)

    def test_pause_resume_and_terminate(self):
        name = self.launch("lifecycle")
        self.check_lifecycle(name)

    def test_a_switch_to_another_account_keeps_the_conversation_folder_and_name(self):
        self.check_switch_account("switch")

    def test_a_team_swap_moves_the_lead_through_u53(self):
        """U54: one owner action moves a team's lead to another account through the
        same U53 switch, keeping its session, conversation and name."""
        name = self.launch("swap")
        self.deliver(name, MESSAGES[0])
        conversation = self.ready(name)["conversation_id"]
        self.second_account()
        team = self.json("team", "create", "--json", input=json.dumps({
            "request_id": str(uuid.uuid4()), "name": "Swap", "by": "human",
            "lead": {"id": "lead", "role": "Lead", "agent": "claude",
                     "session": {"name": name, "host": ""}}}))
        receipt = self.json("team", "swap-member", "--json", input=json.dumps({
            "request_id": str(uuid.uuid4()), "team": team["team"], "by": "human",
            "from_account": "native-claude", "to_account": "second", "provider": "claude"}))
        self.assertEqual(receipt["status"], "swapped", receipt)
        self.assertEqual([entry["member"] for entry in receipt["members"]], ["lead"], receipt)
        # The same session, conversation and name, now on the second account.
        switched = self.record(name)
        self.assertEqual((switched["name"], switched["conversation_id"], switched["account_id"]),
                         (name, conversation, "second"))
        # The roster reads the real account from the session.
        status = self.json("team", "status", "--json", input=json.dumps({"team": team["team"], "by": "human"}))
        lead = next(member for member in status["members"] if member["id"] == "lead")
        self.assertEqual(lead["session_account"], "second", lead)
        # The moved lead still answers in that conversation.
        text = "After the team swap."
        self.deliver(name, text)
        self.assertEqual(self.accepted(name, text, conversation), 1, self.describe())

    def test_input_keys_reach_claude_through_the_host(self):
        self.check_keys(self.launch("keys"))

    def deliver_prompt(self, name, text):
        """Send a prompt whose turn waits for an answer; its receipt."""
        row = self.ready(name)
        receipt = self.send(name, self.message(row, text))
        self.assertEqual(receipt["status"], "submitted", receipt)
        return receipt

    # The answer channel.

    def channels(self, stage):
        """The answer channel files named `stage` in the case's state."""
        return sorted((self.state / "native_answers").glob(f"*/*/{stage}"))

    def channel_card(self, name, source):
        """The pending card of `source` once the answer channel waits for it."""
        return self.until(f"a {source} card of the answer channel", lambda: next(
            (card for card in self.inspect(name).get("pending_questions", [])
             if card.get("source") == source and card.get("answer_transport") == "claude_hook"
             and card.get("can_answer")), None))

    def hook_card(self, name, prompt):
        """Send `prompt`, whose Bash call waits for approval; its card once the answer channel waits."""
        self.deliver_prompt(name, prompt)
        return self.channel_card(name, "claude_tool_approval")

    def worker(self):
        """The (PID, start) of the answer channel that waits now."""
        def waiting():
            for path in self.channels("waiting.json"):
                stages = {entry.name for entry in path.parent.iterdir()}
                if "closed.json" not in stages:
                    record = json.loads(path.read_text(encoding="utf-8"))
                    if self.alive(record["pid"], record["process_start"]):
                        return record["pid"], record["process_start"]
            return None
        return self.until("the waiting answer channel", waiting)

    def choose(self, card, label):
        ids = [option["id"] for option in card["questions"][0]["options"] if option["label"] == label]
        self.assertEqual(len(ids), 1, card["questions"][0]["options"])
        return [dict(question_id="approval", selected_option_ids=ids, text="")]

    def refused(self, name, card, answers):
        """`hgs answer` for a card that is no longer pending fails and sends nothing."""
        row = self.row(name)
        payload = dict(request_id=str(uuid.uuid4()), expected_run_id=row["run_id"],
                       expected_conversation_id=row.get("conversation_id") or "", question_id=card["question_id"],
                       expected_question_hash=card["question_hash"], answers=answers)
        result = self.hgs("answer", name, "--json", input=json.dumps(payload), rc=None)
        self.assertNotEqual(result.returncode, 0, result.stdout)


@unittest.skipUnless(CODEX and HGS_SET, "set HGS_TEST_BIN and HGS_CODEX_TEST_BIN to run real Codex")
class NativeWindowsCodex(NativeAgent, host.HostCase):
    AGENT = "codex"
    BINARY = CODEX
    # Spike S6's arguments, beside full access and no approvals in its home: no sandbox.
    ARGS = ("--dangerously-bypass-approvals-and-sandbox",)
    COMPOSER = "›"
    # Codex asks to review the hooks hgs installs; only the approval case answers that.
    NO_HOOK_REVIEW = "--dangerously-bypass-hook-trust"

    def configure(self):
        trusted = [f"[projects.{json.dumps(form)}]\ntrust_level = \"trusted\"\n" for form in path_forms(self.project)]
        (self.agent_home / "config.toml").write_text("\n".join([
            "# A private CODEX_HOME: the provider is tests/model_api_fixture.py on 127.0.0.1.",
            'model = "gpt-6.1-sol"', 'model_provider = "fixture"', "check_for_update_on_startup = false",
            'sandbox_mode = "danger-full-access"', 'approval_policy = "never"', "",
            # Spike S3 recorded Codex 0.162.0's asynchronous questions with this feature on;
            # 0.162.1 offers request_user_input_async either way.
            "[features]", "send_message_to_user_async = true", "",
            "[model_providers.fixture]", 'name = "Fixture"', f'base_url = "{self.api_url}/v1"',
            'env_key = "OPENAI_API_KEY"', 'wire_api = "responses"', "request_max_retries = 0",
            "stream_max_retries = 0", "", *trusted]), encoding="utf-8")
        self.env["CODEX_HOME"] = str(self.agent_home)

    def prepare_account(self, folder):
        """A managed account's session drops OPENAI_API_KEY (accounts::select), so its
        provider reads the fixture's key from a variable that is its own."""
        config = (self.agent_home / "config.toml").read_text(encoding="utf-8")
        (folder / "config.toml").write_text(
            config.replace('env_key = "OPENAI_API_KEY"', 'env_key = "FIXTURE_API_KEY"'), encoding="utf-8")

    def test_native_subagent_keeps_the_parent_transcript_and_accepts_more_input(self):
        name = self.launch("child", self.NO_HOOK_REVIEW, "--enable", "multi_agent")
        self.deliver(name, "Parent fixture before spawning.")
        parent = self.record(name)
        self.assertTrue(parent.get("conversation_id"), self.describe())
        self.assertTrue(parent.get("transcript"), self.describe())
        self.deliver(name, api.CODEX_SUBAGENT)
        outputs = [result for entry in api.requests(self.api_root) if entry.get("prompt") == api.CODEX_SUBAGENT
                   for result in entry.get("results") or []]
        self.assertFalse(any(isinstance(result, str) and result.startswith(
            ("unsupported call:", "failed to parse function arguments:")) for result in outputs), outputs)

        def child_finished():
            record = self.record(name)
            for key in ("conversation_id", "transcript", "run_id", "pid"):
                self.assertEqual(record.get(key), parent[key], key)
            children = record.get("subagents") or {}
            return next(((sid, child) for sid, child in children.items()
                         if child.get("state") == "finished" and not child.get("external")), None)

        sid, child = self.until("the native child's separate result", child_finished)
        self.assertEqual(child["detail"], api.REPLY)
        paths = list((self.agent_home / "sessions").rglob(f"*{sid}.jsonl"))
        self.assertEqual(len(paths), 1)
        with paths[0].open(encoding="utf-8") as transcript:
            meta = json.loads(transcript.readline())["payload"]
        self.assertIn("subagent", meta["source"])
        self.assertEqual(meta["parent_thread_id"], parent["conversation_id"])
        self.deliver(name, "Parent fixture after spawning.")
        self.assertEqual(self.record(name)["conversation_id"], parent["conversation_id"])
        for text in ("Parent fixture before spawning.", api.CODEX_SUBAGENT, "Parent fixture after spawning."):
            self.assertEqual(self.accepted(name, text, parent["conversation_id"]), 1)

    def test_permission_combinations_match_the_native_argument_parser(self):
        # Completion parses the real installed CLI arguments and exits without
        # starting an agent, requesting a model or setting up a Windows sandbox.
        cases = [
            ({}, [], True),
            ({"approve_for_me": "on"}, ["--approve-for-me"], True),
            ({"approval": "on-request", "sandbox": "workspace-write"},
             ["-a", "on-request", "-s", "workspace-write"], True),
        ]
        for key, flag, value in (("approval", "-a", "on-request"), ("approval", "-a", "never"),
                                 ("sandbox", "-s", "workspace-write"), ("sandbox", "-s", "read-only")):
            cases.append(({"approve_for_me": "on", key: value}, ["--approve-for-me", flag, value], False))
        for selection, flags, valid in cases:
            with self.subTest(selection=selection):
                native = subprocess.run([str(self.BINARY), *flags, "completion", "bash"], env=self.env,
                                        capture_output=True, encoding="utf-8", errors="replace", timeout=30,
                                        creationflags=host.HIDDEN, stdin=subprocess.DEVNULL)
                self.assertEqual(native.returncode, 0 if valid else 2, native.stderr)
                mode = "native:" + json.dumps(selection, separators=(",", ":"))
                planned = self.hgs("codex", str(self.project), "--dry-run", "-d", "--permissions", mode,
                                   rc=0 if valid else 1)
                if not valid:
                    self.assertIn("cannot be combined", planned.stderr)
                elif selection.get("approve_for_me"):
                    self.assertIn("codex --approve-for-me", planned.stdout)

    def transcript_rows(self, conversation_id):
        # After a switch the conversation lives in the new account's store; the
        # old account keeps only its own copy of the same rows.
        homes = ([self.second_home] if self.second_home else []) + [self.agent_home]
        paths = []
        for home in homes:
            paths = sorted((home / "sessions").rglob(f"rollout-*{conversation_id}.jsonl"))
            if paths:
                break
        rows = []
        for path in paths:
            for line in path.read_text(encoding="utf-8").splitlines():
                try:
                    rows.append(json.loads(line))
                except ValueError:
                    pass  # A line still being written.
        return rows

    def check_hook_form(self):
        """The shell form as `command` and, as Codex runs hooks with PowerShell on Windows,
        the PowerShell form as `commandWindows` (spec D12)."""
        hooks = json.loads((self.agent_home / "hooks.json").read_text(encoding="utf-8"))
        stable = os.path.realpath(self.bin / "hgs.exe").replace(os.sep, "/")
        self.assertEqual(hooks["hooks"]["Stop"], [{"hooks": [{
            "type": "command", "command": f'"{stable}" __state hook',
            "commandWindows": f'& "{stable}" __state hook; exit $LASTEXITCODE', "timeout": 5}]}])

    def user_messages(self, conversation_id):
        """The user messages Codex gave its model in a conversation, as their text."""
        messages = []
        for row in self.transcript_rows(conversation_id):
            payload = row.get("payload") if isinstance(row.get("payload"), dict) else {}
            if row.get("type") == "response_item" and (payload.get("type"), payload.get("role")) == ("message", "user"):
                messages.append("".join(part.get("text", "") for part in payload.get("content") or []
                                        if isinstance(part, dict) and part.get("type") == "input_text"))
        return messages

    def accepted(self, name, text, conversation_id=None):
        conversation_id = conversation_id or self.row(name).get("conversation_id")
        return self.user_messages(conversation_id).count(text) if conversation_id else 0

    def turn_ended(self, name, since):
        # inspect takes the writer lock also needed by the native five-second
        # Stop hook. New sessions journal by durable ID, and a renamed legacy
        # session retains its original journal; neither key is its current label.
        # Do not repeatedly launch inspectors ahead of that hook on
        # a busy runner. Observe its journal read-only, then retain the public
        # Stop/idle assertions; a missing or timed-out hook still fails the test.
        record = self.record(name)
        run, conversation = record.get("run_id"), record.get("conversation_id")
        if not run or not conversation:
            return None
        path = self.state / "events.sqlite3"
        try:
            with closing(sqlite3.connect(f"file:{path.as_posix()}?mode=ro", uri=True, timeout=0)) as database:
                stopped = database.execute(
                    "SELECT 1 FROM events WHERE name=? AND conversation=? "
                    "AND json_extract(payload, '$.run_id')=? "
                    "AND json_extract(payload, '$.type')='Stop' "
                    "AND COALESCE(json_extract(payload, '$.agent_id'), '') IN ('', 'main') "
                    "AND json_extract(payload, '$.at')>=? ORDER BY seq DESC LIMIT 1",
                    (record.get("journal_name") or name, conversation, run, since)).fetchone()
        except sqlite3.OperationalError:
            return None  # Not created yet or momentarily busy; the original end still applies.
        return super().turn_ended(name, since) if stopped else None

    def test_project_powershell_wrapper_reports_native_hooks_and_exact_messages(self):
        program = str(self.bin / "codex.exe").replace("'", "''")
        (self.project / "run_codex.ps1").write_text(f"& '{program}' @args; exit $LASTEXITCODE\n", encoding="utf-8-sig")
        name = self.launch("wrapper", self.NO_HOOK_REVIEW)
        self.check_messages(name)
        self.assertTrue(self.inspect(name)["tracked"])
        self.check_hook_form()

    def test_a_tracked_launch_reports_its_hooks_and_send_delivers_exact_messages(self):
        name = self.launch("messages", self.NO_HOOK_REVIEW)
        # Codex 0.162 starts its conversation with the first message, whose hooks confirm it;
        # hgs sends that message to the unconfirmed session (`first_message_can_send`).
        self.check_messages(name)
        row = self.row(name)
        rollout = next((self.agent_home / "sessions").rglob(f"rollout-*{row['conversation_id']}.jsonl"))
        self.assertTrue(os.path.samefile(self.record(name)["transcript"], rollout))
        self.assertTrue(self.inspect(name)["tracked"])
        self.check_hook_form()
        events = [event["type"] for event in self.inspect(name)["events"]]
        self.assertTrue({"SessionStart", "UserPromptSubmit", "Stop"} <= set(events), events)
        journal = self.journal(row["run_id"])
        self.assertEqual(journal.count("UserPromptSubmit"), len(MESSAGES), journal)

    def test_literal_at_mention_is_not_replaced_by_a_file(self):
        (self.project / 'literalreviewer.log').write_text('Synthetic completion target.\n')
        name = self.launch('at-mention', self.NO_HOOK_REVIEW)
        text = 'Send a fixture test to @literalreviewer'
        payload, receipt = self.deliver(name, text)
        self.assertEqual(self.accepted(name, text), 1, self.describe())
        self.assertEqual(self.send(name, payload), receipt)
        self.deliver(name, 'After the literal mention.')
        self.assertEqual(self.accepted(name, text), 1, self.describe())

    def test_hgs_answers_request_user_input_async(self):
        (self.api_root / 'hold-codex-questions').touch()
        name = self.launch("question", self.NO_HOOK_REVIEW)
        # Keep a genuine question alive. Once its turn ends, Codex clears it;
        # answering a stale card from an idle composer is not question coverage.
        row = self.ready(name)
        self.send(name, self.message(row, api.QUESTIONS))
        self.until('the native optional question', lambda: any(
            card.get('source') == 'codex_async' for card in self.inspect(name).get('pending_questions', [])))
        self.keys(name, {'named': 'S-Left'})
        card = self.card(name, "codex_async")
        item, = card["questions"]
        self.assertEqual((item["question"], [option["label"] for option in item["options"]]),
                         (api.CODEX_QUESTIONS[0]["title"], api.CODEX_QUESTIONS[0]["options"]))
        # hgs waits 6 s for Codex to record the answer, then reports it `submitted`: delivered,
        # not yet confirmed (codex_questions::answer). Windows can take longer; the transcript
        # below confirms it either way.
        payload, receipt = self.answer(name, card, [dict(question_id="q_0", selected_option_ids=["opt_1"], text="")],
                                       ("answered", "submitted"))
        replay = self.json("answer", name, "--json", input=json.dumps(payload))
        self.assertEqual({**replay, "status": receipt["status"]}, receipt)
        self.assertIn(replay["status"], ("answered",) if receipt["status"] == "answered" else ("answered", "submitted"), replay)
        # Codex hands the answer on as a user message naming the native question.
        tag = "send_user_message_question_reply"
        reply, = self.until("the answer in the transcript", lambda: [
            text for text in self.user_messages(card["conversation_id"]) if f"<{tag}>" in text])
        body = reply.split(f"<{tag}>", 1)[1].split(f"</{tag}>", 1)[0]
        self.assertEqual(json.loads(body), [{"questionItemId": card["native_question_id"],
                                             "question": item["question"], "answer": "Two"}])
        self.until("the answered card to go", lambda: not [
            card for card in self.inspect(name).get("pending_questions", []) if card.get("source") == "codex_async"])

    def test_optional_questions_close_when_codex_finishes_the_turn(self):
        name = self.launch('question-end', self.NO_HOOK_REVIEW)
        self.deliver(name, api.QUESTIONS)
        self.assertEqual(self.inspect(name)['pending_questions'], [], self.describe())
        self.assertEqual(self.row(name)['optional_questions']['count'], 0, self.describe())
        self.assertFalse(any('<send_user_message_question_reply>' in text
                             for text in self.user_messages(self.record(name)['conversation_id'])))

    def test_hgs_trusts_the_hooks_it_installed(self):
        # Without the bypass Codex stops at its review of the new hooks before SessionStart.
        name = self.launch("approval")
        card = self.card(name, "codex_hooks_trust")
        self.assertEqual([option["id"] for option in card["questions"][0]["options"]],
                         ["review", "trust", "continue_without_trusting"])
        self.answer(name, card, [dict(question_id="hooks_trust", selected_option_ids=["trust"], text="")])
        # The trusted hooks run: the first message's hooks confirm the conversation.
        self.deliver(name, MESSAGES[0])
        row = self.row(name)
        self.assertTrue(row.get("conversation_id"), row)
        self.assertEqual(self.accepted(name, MESSAGES[0]), 1, self.describe())
        events = [event["type"] for event in self.inspect(name)["events"]]
        self.assertTrue({"SessionStart", "UserPromptSubmit", "Stop"} <= set(events), events)

    def tool_approval_fixture(self, automatic):
        # Escalation goes straight to the permission flow, without setting up
        # or executing inside a Windows sandbox. Nothing uses a real reviewer
        # account: the automatic case exercises Codex's documented allow hook.
        self.ARGS = ()
        config = self.agent_home / "config.toml"
        config.write_text(config.read_text(encoding="utf-8")
                          .replace('approval_policy = "never"', 'approval_policy = "on-request"')
                          .replace('sandbox_mode = "danger-full-access"', 'sandbox_mode = "read-only"'),
                          encoding="utf-8")
        if automatic:
            decision = json.dumps({"hookSpecificOutput": {"hookEventName": "PermissionRequest",
                                                        "decision": {"behavior": "allow"}}})
            # Leave time for listing polls while the native approval decision
            # is still automatic and its tracking hook has already arrived.
            command = "Start-Sleep -Seconds 2; Write-Output '" + decision + "'"
            (self.agent_home / "hooks.json").write_text(json.dumps({"hooks": {"PermissionRequest": [
                {"hooks": [{"type": "command", "commandWindows": command, "command": command}]}]}}),
                encoding="utf-8")
        name = self.launch("tool-auto" if automatic else "tool-manual", self.NO_HOOK_REVIEW)
        self.send(name, self.message(self.ready(name), api.CODEX_ESCALATION))
        if automatic:
            def started_without_attention():
                view = self.row(name)
                self.assertNotEqual(view["phase"], "approval", view)
                return (self.project / api.CODEX_STARTED).is_file()
            self.until("automatic approval without a human-attention phase", started_without_attention)
        return name

    def check_continuing_permission(self, name):
        self.until("the approved native command to start", lambda: (self.project / api.CODEX_STARTED).is_file())
        # Codex sent PermissionRequest but has not sent PostToolUse. Waiting
        # for the command to finish would leave a false approval toast up.
        record = self.record(name)
        self.assertEqual(record["phase"], "approval", self.describe())
        self.assertTrue(any(event["type"] == "PermissionRequest" for event in self.inspect(name)["events"]))
        for _ in range(3):
            for view in (self.row(name), self.inspect(name)):
                self.assertEqual(view["phase"], "tool", view)
                self.assertEqual(view["session_state"], "working", view)
            time.sleep(0.1)
        (self.project / api.CODEX_RELEASE).touch()
        self.until("the approved command's turn to finish", lambda: self.turn_ended(name, 0))

    def test_a_hook_allowed_codex_command_does_not_request_human_approval(self):
        self.check_continuing_permission(self.tool_approval_fixture(automatic=True))

    def test_codex_model_review_does_not_request_human_approval(self):
        self.ARGS = ()
        config = self.agent_home / "config.toml"
        config.write_text(config.read_text(encoding="utf-8")
                          .replace('approval_policy = "never"', 'approval_policy = "on-request"\n'
                                   'approvals_reviewer = "auto_review"')
                          .replace('sandbox_mode = "danger-full-access"', 'sandbox_mode = "read-only"'),
                          encoding="utf-8")
        name = self.launch("tool-review", self.NO_HOOK_REVIEW)
        self.send(name, self.message(self.ready(name), api.CODEX_ESCALATION))
        try:
            self.until("the local model reviewer", lambda: (self.project / api.CODEX_REVIEW_STARTED).is_file())
            self.until("the native review status with command details",
                       lambda: "Reviewing approval request" in self.capture(name) and "└" in self.capture(name))
            self.assertFalse((self.project / api.CODEX_STARTED).exists())
            self.assertEqual(self.record(name)["phase"], "approval")
            for _ in range(3):
                for view in (self.row(name), self.inspect(name)):
                    self.assertEqual((view["phase"], view["session_state"]), ("tool", "working"), view)
        finally:
            (self.project / api.CODEX_REVIEW_RELEASE).touch()
        self.check_continuing_permission(name)

    def test_a_codex_terminal_approval_clears_before_the_command_finishes(self):
        name = self.tool_approval_fixture(automatic=False)
        self.until("the native permission chooser", lambda: "Would you like to run the following command?" in self.capture(name))
        for view in (self.row(name), self.inspect(name)):
            self.assertEqual((view["phase"], view["session_state"]), ("approval", "user"), view)
        self.assertFalse((self.project / api.CODEX_STARTED).exists())
        self.keys(name, {"named": "Enter"})
        self.check_continuing_permission(name)

    def untrusted_folder(self, git=False):
        config = self.agent_home / 'config.toml'
        config.write_text(config.read_text(encoding='utf-8').split('[projects.', 1)[0], encoding='utf-8')
        target = self.project.parent if git else self.project
        (target / '.codex').mkdir()
        (target / '.codex/config.toml').write_text('# Synthetic project, no hooks or commands.\n', encoding='utf-8')
        if git:
            subprocess.run(['git', 'init', '-q', str(target)], check=True, env=self.env, timeout=15)
        name = self.launch('folder-trust', self.NO_HOOK_REVIEW)
        card = self.card(name, 'codex_folder_trust')
        self.assertNotIn('projects', tomllib.loads(config.read_text(encoding='utf-8')))
        self.assertFalse(self.record(name).get('conversation_id'))
        return name, card, target

    def test_explicit_folder_trust_names_the_git_root_then_sends(self):
        name, card, target = self.untrusted_folder(git=True)
        question = card['questions'][0]
        self.assertEqual(question['question'], 'Trust this Git repository?')
        self.assertIn(str(target).lower(), question['options'][0]['description'].lower())
        payload, receipt = self.answer(name, card, [dict(question_id='trust', selected_option_ids=['trust_0'], text='')])
        self.assertEqual(self.json('answer', name, '--json', input=json.dumps(payload)), receipt)
        projects = tomllib.loads((self.agent_home / 'config.toml').read_text(encoding='utf-8'))['projects']
        self.assertEqual({path.lower() for path, value in projects.items() if value['trust_level'] == 'trusted'},
                         {str(target).lower()})
        # Windows 0.167 opens a separate sandbox NUX after folder consent,
        # even with this fixture's YOLO launch. Trust succeeded; the following
        # chooser must still block input. Never set up a sandbox in this test.
        notice = 'Set up the Codex agent sandbox'
        self.until('the separate Windows sandbox setup chooser', lambda: notice in self.capture(name))
        blocked = self.hgs('send', name, '--json', input=json.dumps(self.message(self.row(name), MESSAGES[0])), rc=None)
        self.assertNotEqual(blocked.returncode, 0, blocked.stdout)
        self.assertIn('terminal has a draft, dialog, or unrecognized prompt', blocked.stderr)
        self.assertEqual(api.requests(self.api_root), [])
        self.assertFalse(any(q.get('source') == 'codex_folder_trust'
                             for q in self.inspect(name).get('pending_questions', [])))
        # This is the fixture user's Terminal action, not an app trust answer
        # or a sandbox/Administrator choice. The same process then takes input.
        self.keys(name, {'named': 'Escape'})
        self.until('sandbox setup dismissed in Terminal', lambda: notice not in self.capture(name))
        self.assertIsNone(tomllib.loads((self.agent_home / 'config.toml').read_text(encoding='utf-8')).get('windows', {}).get('sandbox'))
        self.deliver(name, MESSAGES[0])

    def test_explicit_folder_cancel_keeps_trust_unchanged(self):
        name, card, _ = self.untrusted_folder()
        cancel = card['questions'][0]['options'][1]['label']
        self.assertIn(cancel, ('Quit', 'Back to Agent Command Center'))
        payload, receipt = self.answer(name, card, [dict(question_id='trust', selected_option_ids=['trust_1'], text='')])
        self.assertEqual(self.json('answer', name, '--json', input=json.dumps(payload)), receipt)
        if cancel == 'Back to Agent Command Center':
            self.until('Back to the live command center', lambda: 'Agent command center' in self.capture(name))
            self.assertEqual(self.row(name)['process_state'], 'running')
        self.assertNotIn('projects', tomllib.loads((self.agent_home / 'config.toml').read_text(encoding='utf-8')))
        self.assertFalse(self.record(name).get('conversation_id'))

    def test_clear_context_starts_a_new_conversation_in_the_same_agent(self):
        name = self.launch("clear", self.NO_HOOK_REVIEW)
        self.deliver(name, MESSAGES[0])
        self.settled(name)
        # Its header and folder still show above the first message, which is no reset panel.
        self.assertIsNone(self.inspect(name)["session_clear"],
                          "hgs took Codex's header above a message for its reset panel (zerus-g9dd)" + self.describe())
        # Codex shows its reset panel at once and reports the new conversation with the next
        # message's SessionStart. hgs confirms the panel when it names the project folder as
        # Codex prints it, which below the real profile is a `~\…` form of another home.
        outside = not below(self.project, os.environ.get("USERPROFILE") or os.path.expanduser("~"))
        row, record, _ = self.check_clear(name, "confirmed" if outside else None)
        if outside:
            self.assertEqual(self.inspect(name)["session_clear"]["type"], "SessionCleared")
        self.check_after_clear(name, row, record)
        self.assertEqual(self.inspect(name)["session_clear"]["type"], "SessionCleared")

    def test_pause_resume_and_terminate(self):
        self.env['RUST_LOG'] = 'warn,codex_core::rollout=debug,codex_state=debug,codex_tui::session_resume=debug'
        name = self.launch("lifecycle", self.NO_HOOK_REVIEW)
        try:
            self.check_lifecycle(name)
        finally:
            diagnostic = getattr(self, 'resume_diagnostic', None)
            if diagnostic is not None:
                print('ZERUS_CODEX_RESUME_EVIDENCE=' + diagnostic.finish(self.agent_home), flush=True)

    def test_a_switch_to_another_account_keeps_the_conversation_folder_and_name(self):
        self.check_switch_account("switch")

    def test_input_keys_reach_codex_through_the_host(self):
        self.check_keys(self.launch("keys", self.NO_HOOK_REVIEW))

    def test_ultra_composer_accepts_exact_input_and_clear_without_a_restart(self):
        self.COMPOSER = "»"
        name = self.launch("ultra", self.NO_HOOK_REVIEW, "-c", 'model_reasoning_effort="ultra"')
        self.deliver(name, MESSAGES[0])
        self.settled(name)
        self.assertTrue(any(row.startswith("»") for row in self.capture(name).splitlines()), self.describe())
        outside = not below(self.project, os.environ.get("USERPROFILE") or os.path.expanduser("~"))
        row, record, _ = self.check_clear(name, "confirmed" if outside else None)
        self.check_after_clear(name, row, record)
