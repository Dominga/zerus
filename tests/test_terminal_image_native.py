"""Opt-in latest-official native literal-paste contracts, using GUI wire DATA.

The GUI's real stage/fake-PTY test supplies the packet while its target file is
still retained. This never touches a resident agent, account or clipboard.
Only private localhost providers, an inert tool and an optional maintained
private tmux/raw-agent transport fixture exist. Prompt acceptance does not prove
that a provider recognized image content.
"""
import base64
import hashlib
import http.server
import json
import os
from pathlib import Path
import re
import select
import struct
import subprocess
import tempfile
import threading
import time
import unittest
import uuid

if os.name == "posix":
    import fcntl
    import pty
    import termios

RECEIPT = os.environ.get("HGS_TERMINAL_IMAGE_CODEX_RECEIPT")
PACKET = os.environ.get("HGS_TERMINAL_IMAGE_PACKET")
HGS = os.environ.get("HGS_TEST_BIN")
OTHER_RECEIPTS = os.environ.get("HGS_TERMINAL_IMAGE_OTHER_RECEIPTS")
COMPOSITION = os.environ.get("HGS_TERMINAL_IMAGE_TMUX_COMPOSITION")
PROMPT = "Request only the synthetic private tool approval."


def offered(items):
    names = set()
    for item in items:
        if isinstance(item, dict):
            if item.get("name"):
                names.add(item["name"])
            names.update(offered(item.get("tools") or []))
    return names


class OwnedTTY:
    """One original child and its controlling PTY, with the existing CLI VT model."""
    def __init__(self, argv, env, cwd, cols, rows):
        self.env, self.cols, self.rows = env, cols, rows
        self.output, self.replies = bytearray(), 0
        self.master, slave = pty.openpty()
        fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", rows, cols, 0, 0))
        self.argv = ["/usr/bin/python3", "-c", "import fcntl,termios,os,sys;fcntl.ioctl(0,termios.TIOCSCTTY,0);os.execv(sys.argv[1],sys.argv[1:])", *map(str, argv)]
        try:
            self.child = subprocess.Popen(self.argv, stdin=slave, stdout=slave, stderr=slave,
                                          env=env, cwd=cwd, start_new_session=True)
        except BaseException:
            os.close(self.master)
            raise
        finally:
            os.close(slave)

    def screen(self):
        while select.select([self.master], [], [], 0)[0]:
            try:
                data = os.read(self.master, 65536)
            except OSError:
                break
            if not data:
                break
            self.output.extend(data)
            if len(self.output) > 2 * 1024 * 1024:
                raise AssertionError("owned native output exceeds bounded capture")
        result = subprocess.run([HGS, "__screen-capture", "--rows", str(self.rows), "--cols", str(self.cols), "--json"],
                                input=self.output, env=self.env, capture_output=True, check=True, timeout=5)
        value = json.loads(result.stdout)
        replies = base64.b64decode(value["replies"])
        if len(replies) > self.replies:
            os.write(self.master, replies[self.replies:])
            self.replies = len(replies)
        return base64.b64decode(value["plain"]).decode()

    def close(self):
        os.close(self.master)  # original terminal EOF first, never a by-name process stop
        try:
            self.child.wait(timeout=3)
        except subprocess.TimeoutExpired:
            self.child.terminate()
            try:
                self.child.wait(timeout=5)
            except subprocess.TimeoutExpired:
                self.child.kill()
                self.child.wait(timeout=5)


def private_environment(root):
    env = dict(PATH="/usr/bin:/bin", HOME=str(root), TERM="xterm-256color", LANG="C.UTF-8", LC_ALL="C.UTF-8",
               NO_PROXY="127.0.0.1,localhost", NO_AT_BRIDGE="1", DISABLE_AUTOUPDATER="1")
    for key, leaf in [("XDG_CONFIG_HOME", "config"), ("XDG_CACHE_HOME", "cache"), ("XDG_STATE_HOME", "state"),
                      ("XDG_DATA_HOME", "data"), ("XDG_RUNTIME_DIR", "runtime"), ("TMPDIR", "tmp")]:
        (root / leaf).mkdir(mode=0o700)
        env[key] = str(root / leaf)
    return env


@unittest.skipUnless(os.name == "posix" and OTHER_RECEIPTS and PACKET and HGS, "explicit latest Claude/Kimi and GUI packet inputs required")
class NativeTerminalImagePrompts(unittest.TestCase):
    def test_latest_claude_and_kimi_real_prompts_keep_literal_data_without_submit(self):
        receipts = json.loads(Path(OTHER_RECEIPTS).read_text())
        packet = json.loads(Path(PACKET).read_text())
        self.assertEqual(hashlib.sha256(Path(packet["image_file"]).read_bytes()).hexdigest(), packet["image_sha256"])
        for agent, mode, cols, rows in [("claude", "manual", 120, 45), ("kimi", "regular", 80, 24), ("kimi", "fullscreen", 80, 24)]:
            with self.subTest(agent=agent, mode=mode):
                receipt = receipts[agent]
                with Path(receipt["native"]).open("rb") as source:
                    self.assertEqual(hashlib.file_digest(source, "sha256").hexdigest(), receipt["sha256"])
                self.one_case(agent, mode, cols, rows, receipt, packet)

    def one_case(self, agent, mode, cols, rows, receipt, packet):
        with tempfile.TemporaryDirectory(prefix="terminal-image-prompt-") as scratch:
            root = Path(scratch).resolve()
            project = root / "project"
            project.mkdir(mode=0o700)
            model_requests, platform_requests = [], []

            class Provider(http.server.BaseHTTPRequestHandler):
                def log_message(self, *_):
                    pass

                def do_GET(self):
                    if self.path.startswith("/fixture-platform/"):
                        platform_requests.append(self.path)
                        self.send_error(404)
                        return
                    data = b'{"object":"list","data":[{"id":"fixture","object":"model"}]}'
                    self.send_response(200)
                    self.send_header("Content-Type", "application/json")
                    self.send_header("Content-Length", str(len(data)))
                    self.end_headers()
                    self.wfile.write(data)

                def do_POST(self):
                    length = int(self.headers.get("Content-Length", 0))
                    if length > 4 * 1024 * 1024:
                        self.send_error(413)
                        return
                    body = self.rfile.read(length)
                    if self.path.startswith("/fixture-platform/"):
                        platform_requests.append(self.path)
                        self.send_error(404)
                        return
                    model_requests.append(dict(path=self.path, sha256=hashlib.sha256(body).hexdigest()))
                    self.send_error(503, "No model call is admitted by the literal-paste prompt fixture")

            server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Provider)
            serving = threading.Thread(target=server.serve_forever)
            serving.start()
            app = None
            before = after = ""
            preparation = []
            try:
                env = private_environment(root)
                if agent == "claude":
                    (root / ".claude").mkdir(mode=0o700)
                    (root / ".claude.json").write_text(json.dumps(dict(hasCompletedOnboarding=True, hasSeenAutoDefaultNudge=True,
                        theme="dark", customApiKeyResponses=dict(approved=["fixture-key"], rejected=[]),
                        projects={str(project): dict(hasTrustDialogAccepted=True)})))
                    (root / ".claude/settings.json").write_text(json.dumps(dict(permissions=dict(defaultMode="manual"))))
                    env.update(ANTHROPIC_API_KEY="fixture-key", ANTHROPIC_BASE_URL=f"http://127.0.0.1:{server.server_port}",
                               CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC="1")
                    argv = [receipt["native"], "--permission-mode", "manual", "--model", "claude-sonnet-4-6"]
                else:
                    home = root / ".kimi-code"
                    home.mkdir(mode=0o700)
                    (home / "config.toml").write_text('default_model="fixture"\ntelemetry=false\n'
                        '[providers.fixture]\ntype="openai"\nbase_url="http://127.0.0.1:%d/v1"\napi_key="fixture-key"\n'
                        '[models.fixture]\nprovider="fixture"\nmodel="fixture"\nmax_context_size=32768\n'
                        '[loop_control]\nmax_attempts_per_step=1\n' % server.server_port)
                    (home / "tui.toml").write_text(f'tui_mode="{mode}"\n[upgrade]\nauto_install=false\n')
                    env.update(KIMI_CODE_HOME=str(home), KIMI_CODE_BASE_URL=f"http://127.0.0.1:{server.server_port}/fixture-platform",
                               KIMI_CODE_NO_AUTO_UPDATE="1", KIMI_DISABLE_TELEMETRY="1")
                    argv = [receipt["native"]]
                app = OwnedTTY(argv, env, project, cols, rows)
                end = time.monotonic() + 30
                trusted = False
                while time.monotonic() < end:
                    before = app.screen()
                    self.assertIsNone(app.child.poll(), before)
                    if agent == "kimi" and "Trust this folder?" in before:
                        if not trusted:
                            preparation.append(dict(kind="explicit startup trust of fresh empty owned folder", frame=before))
                            os.write(app.master, b"\r")
                            trusted = True
                    elif agent == "claude" and "? for shortcuts" in before and ("manual mode" in before or "manual" in before):
                        break
                    elif agent == "kimi" and "fixture" in before and any(x in before for x in ["❯", "›", ">", "Manual", "MANUAL"]):
                        break
                    time.sleep(0.05)
                else:
                    self.fail("no actual native prompt: " + before)
                self.assertNotIn(".png", before, "observation requires a clean real composer")
                baseline = len(model_requests)
                wire = packet["wire"].encode()
                self.assertEqual(os.write(app.master, wire), len(wire))
                end = min(end, time.monotonic() + 2)
                while time.monotonic() < end:
                    after = app.screen()
                    time.sleep(0.05)
                self.assertEqual(len(model_requests), baseline, "literal image reference submitted a native turn")
                self.assertTrue(".png" in after or "[Image" in after or "[image" in after,
                                "native prompt did not accept the literal reference: " + after)
                if agent == "claude":
                    self.assertIn("manual", after)
                self.assertIsNone(app.child.poll())
                print("NATIVE_LITERAL_PROMPT " + json.dumps(dict(agent=agent, mode=mode, version=receipt["version"],
                    argv=argv, before=before, after=after, new_model_requests=len(model_requests)-baseline,
                    preparation=preparation, wire_sha256=hashlib.sha256(wire).hexdigest())), flush=True)
            finally:
                if app is not None:
                    app.close()
                server.shutdown()
                server.server_close()
                serving.join(5)
                self.assertFalse(serving.is_alive())
                if evidence := os.environ.get("HGS_TERMINAL_IMAGE_NATIVE_EVIDENCE"):
                    destination = Path(evidence)
                    destination.mkdir(parents=True, exist_ok=True)
                    label = f"{agent}-{mode}"
                    raw = bytes(app.output) if app else b""
                    (destination / (label + ".bin")).write_bytes(raw)
                    (destination / (label + ".json")).write_text(json.dumps(dict(version=receipt["version"],
                        package_sha256=receipt["sha256"], before=before, after=after, preparation=preparation,
                        model_requests=model_requests, platform_requests=platform_requests,
                        raw_sha256=hashlib.sha256(raw).hexdigest(), original_child_returncode=app.child.returncode if app else None), indent=2) + "\n")


@unittest.skipUnless(os.name == "posix" and COMPOSITION and PACKET and HGS, "explicit private tmux transport inputs required")
class TerminalImageTmuxComposition(unittest.TestCase):
    def test_actual_attach_preserves_the_one_literal_packet(self):
        # Reuse the maintained private socket/raw-agent fixture, not a resident
        # tmux server. Its original process identity and shutdown checks remain.
        from test_input import InputTransport, TMUX
        self.assertIsNotNone(TMUX, "tmux is required for the selected composition")
        packet = json.loads(Path(PACKET).read_text())
        wire = packet["wire"].encode()
        self.assertTrue(wire.startswith(b"\x1b[200~") and wire.endswith(b"\x1b[201~"))
        self.assertNotIn(b"\r", wire)
        self.assertNotIn(b"\n", wire)
        fixture = InputTransport()
        started = False
        app = None
        try:
            fixture.setUp()
            started = True
            fixture.env["TERM"] = "xterm-256color"
            image = Path(packet["image_file"]).read_bytes()
            self.assertEqual(hashlib.sha256(image).hexdigest(), packet["image_sha256"])
            request = dict(request_id=str(uuid.uuid4()), expected_run_id=fixture.run_id,
                           expected_conversation_id=fixture.conversation_id,
                           attachments=[dict(name="pasted-image.png", mime="image/png",
                                             data_base64=base64.b64encode(image).decode(), reference="")])
            stage = subprocess.run([HGS, "attachment", fixture.name, "--stage", "--json"],
                                   input=json.dumps(request), env=fixture.env,
                                   text=True, capture_output=True, timeout=15)
            self.assertEqual(stage.returncode, 0, stage.stderr)
            receipt = json.loads(stage.stdout)
            self.assertEqual([receipt[k] for k in ["status", "request_id", "name", "run_id", "conversation_id"]],
                             ["staged", request["request_id"], fixture.name, fixture.run_id, fixture.conversation_id])
            self.assertEqual(len(receipt["files"]), 1)
            retained = Path(receipt["files"][0]["path"])
            self.assertTrue(retained.is_relative_to(fixture.state / "attachments"))
            self.assertEqual(retained.read_bytes(), image)
            request.update(request_id=str(uuid.uuid4()), expected_conversation_id="different-fixture-conversation")
            refused = subprocess.run([HGS, "attachment", fixture.name, "--stage", "--json"],
                                     input=json.dumps(request), env=fixture.env,
                                     text=True, capture_output=True, timeout=15)
            self.assertNotEqual(refused.returncode, 0)
            self.assertEqual(retained.read_bytes(), image)
            self.assertEqual(len(list((fixture.state / "attachments").iterdir())), 1)
            app = OwnedTTY([HGS, "a", fixture.name, "--existing", "--run-id", fixture.run_id],
                           fixture.env, fixture.root, 100, 30)
            end = time.monotonic() + 10
            while time.monotonic() < end:
                frame = app.screen()
                self.assertIsNone(app.child.poll(), frame)
                if fixture.tmux("display-message", "-p", "-t", "=" + fixture.name + ":", "#{session_attached}").strip() == "1":
                    break
                time.sleep(.02)
            else:
                self.fail("the exact private attach did not connect")
            self.assertEqual(fixture.received(), b"")
            self.assertEqual(os.write(app.master, wire), len(wire))
            fixture.wait_until(lambda: fixture.received() == wire, 5, lambda: fixture.received().hex())
            # A short bounded quiet observation catches a duplicate key or Enter.
            end = time.monotonic() + .2
            while time.monotonic() < end:
                app.screen()
                self.assertEqual(fixture.received(), wire)
                time.sleep(.02)
            print("TMUX_LITERAL_COMPOSITION " + json.dumps(dict(
                wire_sha256=hashlib.sha256(wire).hexdigest(), bytes=len(wire),
                transport="actual hgs a --existing --run-id and private tmux raw-agent fixture",
                actual_cli_stage_sha256=hashlib.sha256(retained.read_bytes()).hexdigest(),
                stage_receipt=receipt, wrong_conversation_refused=True,
                generic_typing_changed=False, native_agent=False)), flush=True)
        finally:
            if app is not None:
                app.close()
                if evidence := os.environ.get("HGS_TERMINAL_IMAGE_NATIVE_EVIDENCE"):
                    destination = Path(evidence)
                    destination.mkdir(parents=True, exist_ok=True)
                    (destination / "tmux-composition.bin").write_bytes(app.output)
                    (destination / "tmux-composition.json").write_text(json.dumps(dict(
                        wire_sha256=hashlib.sha256(wire).hexdigest(), received_hex=fixture.received().hex(),
                        raw_sha256=hashlib.sha256(app.output).hexdigest(),
                        original_child_returncode=app.child.returncode), indent=2))
            try:
                if started:
                    fixture.tearDown()
            finally:
                fixture.doCleanups()


@unittest.skipUnless(os.name == "posix" and RECEIPT and PACKET and HGS, "explicit sealed Unix Codex/GUI packet/CLI inputs required")
class NativeTerminalImageCodex(unittest.TestCase):
    def test_image_path_never_answers_a_pending_tool_approval(self):
        receipt = json.loads(Path(RECEIPT).read_text())
        native = Path(receipt["native"])
        self.assertEqual(hashlib.sha256(native.read_bytes()).hexdigest(), receipt["sha256"])
        self.assertEqual(receipt["version"], "codex-cli " + receipt["package"]["version"])
        self.assertEqual(hashlib.sha256(Path(HGS).read_bytes()).hexdigest(), os.environ["HGS_TERMINAL_IMAGE_CLI_SHA256"])
        packet = json.loads(Path(PACKET).read_text())
        image = Path(packet["image_file"])
        self.assertEqual(hashlib.sha256(image.read_bytes()).hexdigest(), packet["image_sha256"])
        wire = packet["wire"].encode()
        self.assertNotIn(b"\r", wire)
        self.assertNotIn(b"\n", wire)
        self.assertNotIn(b"\x16", wire)
        self.rows = []
        # These are the maintained Codex geometry/animation combinations.
        for cols, rows in [(95, 47), (110, 35)]:
            for animations in [True, False]:
                with self.subTest(cols=cols, rows=rows, animations=animations):
                    self.one_case(native, wire, cols, rows, animations)

    def one_case(self, native, wire, cols, rows, animations):
        with tempfile.TemporaryDirectory(prefix="terminal-image-codex-") as scratch:
            root = Path(scratch).resolve()
            home, project = root / "home", root / "project"
            home.mkdir(mode=0o700)
            project.mkdir(mode=0o700)
            codex_home = home / ".codex"
            codex_home.mkdir(mode=0o700)
            marker = root / "inert-tool-ran"
            tool = root / "inert-image-tool"
            tool.write_text("#!/usr/bin/python3\nfrom pathlib import Path\nPath(" + repr(str(marker))
                            + ").write_text('synthetic tool only')\nprint('synthetic tool only')\n")
            tool.chmod(0o700)
            requests, all_requests, errors = [], [], []

            class Provider(http.server.BaseHTTPRequestHandler):
                def log_message(self, *_):
                    pass

                def do_GET(self):
                    self.send_response(200)
                    self.send_header("Content-Type", "application/json")
                    self.end_headers()
                    self.wfile.write(b'{"data":[]}')

                def do_POST(self):
                    body = json.loads(self.rfile.read(int(self.headers.get("Content-Length", 0))))
                    items = body.get("input") or []
                    tools = offered((body.get("tools") or []) + [entry for entry in items
                                    if isinstance(entry, dict) and entry.get("type") == "additional_tools"])
                    texts = [part.get("text", "") for entry in items if isinstance(entry, dict) and entry.get("role") == "user"
                             for part in entry.get("content") or [] if isinstance(part, dict)]
                    answered = any(isinstance(entry, dict) and entry.get("type") == "function_call_output" and entry.get("call_id") == "image_call"
                                   for entry in items)
                    # The exact main user turn is distinct from the native title request.
                    # The selected release must actually present the builtin exec_command
                    # approval; its advertised code-host tool list is not that proof.
                    targeted = PROMPT in texts
                    all_requests.append(dict(model=body.get("model"), user_texts=texts, targeted=targeted,
                                             tools=sorted(tools), sha256=hashlib.sha256(json.dumps(body, sort_keys=True).encode()).hexdigest()))
                    if targeted:
                        requests.append(body)
                    if targeted and not answered:
                        item = dict(type="function_call", id="fc_image", call_id="image_call", name="exec_command",
                                    arguments=json.dumps(dict(cmd=str(tool), sandbox_permissions="require_escalated",
                                                              justification="Synthetic private image approval control")),
                                    status="completed")
                    else:
                        item = dict(type="message", id="msg_image", role="assistant", status="completed",
                                    content=[dict(type="output_text", text="Synthetic tool observed.", annotations=[])])
                    response = dict(id="resp_" + uuid.uuid4().hex, object="response", status="completed", output=[item],
                                    usage=dict(input_tokens=1, output_tokens=1, total_tokens=2))
                    try:
                        self.send_response(200)
                        self.send_header("Content-Type", "text/event-stream")
                        self.end_headers()
                        for kind, data in [
                            ("response.created", dict(response=dict(response, status="in_progress", output=[]))),
                            ("response.output_item.added", dict(output_index=0, item=dict(item, status="in_progress"))),
                            ("response.output_item.done", dict(output_index=0, item=item)),
                            ("response.completed", dict(response=response)),
                        ]:
                            self.wfile.write(("event: " + kind + "\ndata: " + json.dumps(dict(type=kind, **data)) + "\n\n").encode())
                            self.wfile.flush()
                    except (BrokenPipeError, ConnectionResetError):
                        pass

            server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Provider)
            serving = threading.Thread(target=server.serve_forever)
            serving.start()
            process = None
            master = slave = None
            output = bytearray()
            before = after = ""
            spawn = []
            try:
                (codex_home / "config.toml").write_text(
                    'model="gpt-6.1-sol"\nmodel_provider="fixture"\ncheck_for_update_on_startup=false\n'
                    '[features]\nplugins=false\n[model_providers.fixture]\nname="Fixture"\n'
                    f'base_url="http://127.0.0.1:{server.server_port}/v1"\nwire_api="responses"\n'
                    '[projects.' + json.dumps(str(project)) + ']\ntrust_level="trusted"\n')
                # An allowlist prevents host model/account/display/service settings leaking in.
                env = dict(PATH="/usr/bin:/bin", HOME=str(home), CODEX_HOME=str(codex_home), TERM="xterm-256color",
                           LANG="C.UTF-8", LC_ALL="C.UTF-8", NO_PROXY="127.0.0.1,localhost", NO_AT_BRIDGE="1")
                for key, leaf in [("XDG_CONFIG_HOME", "config"), ("XDG_CACHE_HOME", "cache"),
                                  ("XDG_STATE_HOME", "state"), ("XDG_DATA_HOME", "data"), ("TMPDIR", "tmp")]:
                    (root / leaf).mkdir(mode=0o700)
                    env[key] = str(root / leaf)
                master, slave = pty.openpty()
                fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", rows, cols, 0, 0))
                command = [str(native), "--no-daemon", "-C", str(project), "-a", "on-request", "-s", "read-only",
                           "-c", "tui.animations=" + str(animations).lower(), PROMPT]
                # A fresh exec helper acquires the controlling PTY after setsid. No
                # Python preexec callback runs in a fork of this threaded provider.
                spawn = ["/usr/bin/python3", "-c", "import fcntl,termios,os,sys;fcntl.ioctl(0,termios.TIOCSCTTY,0);os.execv(sys.argv[1],sys.argv[1:])", *command]
                process = subprocess.Popen(spawn, stdin=slave, stdout=slave, stderr=slave, env=env,
                                           cwd=project, start_new_session=True)
                os.close(slave)
                slave = None
                output, replies = bytearray(), 0
                end = time.monotonic() + 30

                def screen():
                    nonlocal replies
                    while select.select([master], [], [], 0)[0]:
                        try:
                            chunk = os.read(master, 65536)
                        except OSError:
                            break
                        if not chunk:
                            break
                        output.extend(chunk)
                        if len(output) > 2 * 1024 * 1024:
                            raise AssertionError("native output exceeded the original bounded capture")
                    capture = subprocess.run([HGS, "__screen-capture", "--cols", str(cols), "--rows", str(rows), "--json"],
                                             input=output, env=env, capture_output=True, timeout=5, check=True)
                    value = json.loads(capture.stdout)
                    response = base64.b64decode(value["replies"])
                    if len(response) > replies:
                        os.write(master, response[replies:])
                        replies = len(response)
                    return base64.b64decode(value["plain"]).decode()

                before, after = "", ""
                while time.monotonic() < end:
                    before = screen()
                    if "Synthetic private image approval control" in before and ("Yes" in before or "proceed" in before):
                        break
                    self.assertIsNone(process.poll(), before)
                    self.assertFalse(errors, errors)
                    time.sleep(0.05)
                else:
                    self.fail("No actual pending native approval: " + before)
                self.assertFalse(marker.exists(), "inert tool ran before the image packet")
                self.assertEqual(len(requests), 1)
                self.assertEqual(os.write(master, wire), len(wire))
                observed_until = min(end, time.monotonic() + 2)
                while time.monotonic() < observed_until:
                    after = screen()
                    if marker.exists() or len(requests) != 1:
                        break
                    time.sleep(0.05)
                row = dict(cols=cols, rows=rows, animations=animations, argv=command,
                           wire_sha256=hashlib.sha256(wire).hexdigest(), before=before, after=after,
                           requests=len(requests), inert_tool_ran=marker.exists(), native_alive=process.poll() is None)
                self.rows.append(row)
                print("NATIVE_IMAGE_CASE " + json.dumps(row), flush=True)
                self.assertFalse(marker.exists(), "image pathname characters approved the native inert tool")
                self.assertEqual(len(requests), 1, "image paste submitted or answered the model turn")
                self.assertIn("Synthetic private image approval control", after, "approval was changed/dismissed")
                choices = lambda text: [line.strip() for line in text.splitlines() if re.search(r"\b[1-9]\.\s", line)]
                self.assertTrue(choices(before), before)
                self.assertEqual(choices(after), choices(before), "image paste changed the selected approval")
                self.assertIsNone(process.poll())
                # A separately chosen cancellation ends only this private test's
                # pending synthetic call. The image packet itself made no choice.
                os.write(master, b"\x1b")
                while time.monotonic() < end:
                    idle = screen()
                    if "Would you like to run" not in idle and "Ask Codex to do anything" in idle:
                        break
                    time.sleep(0.05)
                else:
                    self.fail("private original did not return to its real prompt: " + idle)
                self.assertFalse(marker.exists())
                baseline = len(requests)
                self.assertNotIn(".png", idle, "the input observation needs a clean real prompt")
                self.assertEqual(os.write(master, wire), len(wire))
                until = min(end, time.monotonic() + 2)
                prompt = idle
                while time.monotonic() < until:
                    prompt = screen()
                    if ".png" in prompt or "[Image" in prompt:
                        break
                    time.sleep(0.05)
                self.assertTrue(".png" in prompt or "[Image" in prompt, "real prompt did not accept the literal reference: " + prompt)
                self.assertEqual(len(requests), baseline, "literal reference submitted a native prompt")
                self.assertFalse(marker.exists())
                row["idle_reference_frame"] = prompt
                print("NATIVE_LITERAL_PROMPT " + json.dumps(row), flush=True)
            finally:
                if process is not None and process.poll() is None:
                    process.terminate()  # only this owned original child; never a PID lookup/adoption
                if process is not None:
                    try:
                        process.wait(timeout=5)
                    except subprocess.TimeoutExpired:
                        process.kill()
                        process.wait(timeout=5)
                if master is not None:
                    os.close(master)
                if slave is not None:
                    os.close(slave)
                server.shutdown()
                server.server_close()
                serving.join(5)
                self.assertFalse(serving.is_alive())
                if evidence := os.environ.get("HGS_TERMINAL_IMAGE_NATIVE_EVIDENCE"):
                    destination = Path(evidence)
                    destination.mkdir(parents=True, exist_ok=True)
                    label = f"{cols}x{rows}-{str(animations).lower()}"
                    (destination / (label + ".bin")).write_bytes(output)
                    (destination / (label + ".json")).write_text(json.dumps(dict(
                        argv=spawn, before=before, after=after, inert_tool_ran=marker.exists(),
                        requests=all_requests,
                        raw_sha256=hashlib.sha256(output).hexdigest(), wire_sha256=hashlib.sha256(wire).hexdigest(),
                        errors=errors, original_child_returncode=process.returncode if process else None), indent=2) + "\n")


if __name__ == "__main__":
    classes = [NativeTerminalImageCodex] if RECEIPT else []
    if OTHER_RECEIPTS:
        classes.append(NativeTerminalImagePrompts)
    if COMPOSITION:
        classes.append(TerminalImageTmuxComposition)
    suite = unittest.TestSuite(unittest.defaultTestLoader.loadTestsFromTestCase(cls) for cls in classes)
    result = unittest.TextTestRunner(verbosity=2).run(suite)
    raise SystemExit(0 if classes and result.wasSuccessful() and result.testsRun == len(classes) and not result.skipped else 1)
