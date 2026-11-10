"""Native launch ambiguity, through the actual CLI and a private HTTP service.

The service cannot launch a process. Its POST counter detects an unintended
native creation; every config, record, socket and home is fixture-owned.
"""
import hashlib
import http.server
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import threading
import unittest
import uuid

REPO = Path(__file__).resolve().parents[1]
HGS = Path(os.environ.get("HGS_TEST_BIN", REPO / "target/debug/hgs")).resolve()
OPENAPI = json.loads((REPO / "tests/fixtures/opencode2/openapi-2.0.32.json").read_text())


@unittest.skipIf(sys.platform == "win32", "native providers are supported on Linux/macOS")
class NativeLabelLaunch(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="native-labels-", dir="/tmp")
        self.root = Path(self.temp.name)
        self.workspace = self.root / "workspace"
        self.workspace.mkdir()
        self.state = self.root / "state"
        self.native = self.root / "native"
        self.native.mkdir()
        self.name = "opencode/workspace/shared"
        self.posts = []
        sessions = {}
        posts = self.posts
        class Service(http.server.BaseHTTPRequestHandler):
            def log_message(self, *_):
                pass
            def do_GET(self):
                if self.path == "/api/info":
                    self.reply({"version": "2.0.32"})
                elif self.path == "/openapi.json":
                    self.reply(OPENAPI)
                elif self.path.startswith("/api/session/") and self.path.removeprefix("/api/session/") in sessions:
                    self.reply({"data": sessions[self.path.removeprefix("/api/session/")]})
                else:
                    self.reply({"data": []})
            def do_POST(self):
                value = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
                posts.append((self.path, value))
                sessions[value['id']] = value
                self.reply({"data": {"id": value["id"], "location": value["location"]}})
            def reply(self, value):
                body = json.dumps(value).encode()
                self.send_response(200)
                self.send_header("Content-Length", str(len(body)))
                self.end_headers()
                self.wfile.write(body)
        self.server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Service)
        self.worker = threading.Thread(target=self.server.serve_forever)
        self.worker.start()
        registration = self.root / "xdg-state/opencode/service.json"
        registration.parent.mkdir(parents=True)
        registration.write_text(json.dumps(dict(id="private-fixture", version="2.0.32", pid=os.getpid(),
            url="http://127.0.0.1:" + str(self.server.server_port), password="fixture-only")))
        self.records = self.state / "opencode/sessions"
        self.records.mkdir(parents=True)
        for index in range(2):
            sid = str(uuid.uuid4())
            record = dict(backend="opencode", service="opencode2", agent="opencode", name=self.name,
                session_id=sid, layout="id", journal_name=sid, run_id="run-" + str(index),
                conversation_id="ses_original" + str(index), cwd=str(self.workspace), agent_home=str(self.native))
            (self.records / (sid + ".json")).write_text(json.dumps(record))
        self.env = {k: v for k, v in os.environ.items() if not k.startswith(
            ("HGS_", "TMUX", "XDG_", "ZERUS_", "OPENCODE_", "DSH_"))
            and k not in ("DISPLAY", "WAYLAND_DISPLAY", "DBUS_SESSION_BUS_ADDRESS", "SSH_AUTH_SOCK", "LD_PRELOAD")}
        tmux = self.root / "tmux"
        tmux.mkdir(mode=0o700)
        self.env.update(HOME=str(self.root), HGS_CONFIG_DIR=str(self.root / "config"), HGS_STATE_DIR=str(self.state),
            HGS_SELF="fixture", HGS_PEERS="", OPENCODE_CONFIG_DIR=str(self.native), XDG_STATE_HOME=str(self.root / "xdg-state"),
            XDG_CONFIG_HOME=str(self.root / "xdg-config"), XDG_DATA_HOME=str(self.root / "xdg-data"),
            XDG_CACHE_HOME=str(self.root / "xdg-cache"), TMUX_TMPDIR=str(tmux))

    def tearDown(self):
        self.server.shutdown()
        self.worker.join(timeout=3)
        self.server.server_close()
        self.assertFalse(self.worker.is_alive())
        self.temp.cleanup()

    def refuse(self, new, damaged=False):
        if damaged:
            next(self.records.glob("*.json")).write_text("{damaged")
        before = {p.name: p.read_bytes() for p in self.records.glob("*.json")}
        if not damaged:
            inspected = subprocess.run([str(HGS), "inspect", self.name, "--json"], env=self.env,
                capture_output=True, text=True, timeout=15)
            self.assertEqual(inspected.returncode, 5, inspected.stdout + inspected.stderr)
        result = subprocess.run([str(HGS), "opencode", str(self.workspace), "-n", "shared", "-d", *(["--new"] if new else [])],
            env=self.env, capture_output=True, text=True, timeout=15)
        self.assertEqual(self.posts, [], "ambiguous/unreadable label reached native creation")
        self.assertNotEqual(result.returncode, 0, result.stdout)
        legacy = self.state / "opencode/bindings" / (hashlib.sha256(self.name.encode()).hexdigest() + ".json")
        self.assertFalse(legacy.exists(), "a third legacy binding was created")
        self.assertEqual({p.name: p.read_bytes() for p in self.records.glob("*.json")}, before)

    def test_ordinary_launch_refuses_an_ambiguous_native_label(self):
        self.refuse(False)
    def test_new_launch_refuses_an_ambiguous_native_label(self):
        self.refuse(True)
    def test_ordinary_launch_refuses_unreadable_label_coverage(self):
        self.refuse(False, damaged=True)
    def test_new_launch_refuses_unreadable_label_coverage(self):
        self.refuse(True, damaged=True)

    def test_new_launch_records_proven_parent_and_reopen_keeps_detached_link(self):
        lead = dict(version=1, agent='claude', name='claude/example/lead', session_id=str(uuid.uuid4()),
                    run_id='run-lead', conversation_id='conversation-lead', pid=os.getpid(),
                    process_start=subprocess.check_output(['ps', '-p', str(os.getpid()), '-o', 'lstart='],
                        env=dict(self.env, LC_ALL='C', TZ='UTC'), text=True).strip())
        path = self.state / (hashlib.sha256(lead['name'].encode()).hexdigest() + '.json')
        path.write_text(json.dumps(lead))
        reference = subprocess.run([str(HGS), 'reference', lead['name'], '--json'], env=self.env,
            capture_output=True, text=True, timeout=15)
        self.assertEqual(reference.returncode, 0, reference.stderr)
        env = dict(self.env, HGS_SESSION=lead['name'], HGS_RUN_ID=lead['run_id'])
        def launch(tag, env, new=False):
            result = subprocess.run([str(HGS), 'opencode', str(self.workspace), '-n', tag, '-d', *(['--new'] if new else [])],
                env=env, capture_output=True, text=True, timeout=15)
            self.assertEqual(result.returncode, 0, result.stderr)
            binding = self.state / 'opencode/bindings' / (hashlib.sha256(f'opencode/workspace/{tag}'.encode()).hexdigest() + '.json')
            return binding, json.loads(binding.read_text())
        _, child = launch('linked', env, new=True)
        self.assertEqual(child['parent']['session_id'], lead['session_id'])
        self.assertEqual(child['parent']['source'], 'launch')
        inherited = json.dumps(child['parent'])
        request = dict(expected_run_id=child['run_id'], expected_conversation_id=child['conversation_id'], action='clear')
        result = subprocess.run([str(HGS), 'parent', child['name'], '--json'], env=self.env,
            input=json.dumps(request), capture_output=True, text=True, timeout=15)
        self.assertEqual(result.returncode, 0, result.stderr)
        _, resumed = launch('linked', env)
        self.assertNotIn('parent', resumed)
        self.assertEqual((resumed['session_id'], resumed['conversation_id']), (child['session_id'], child['conversation_id']))
        _, plain = launch('plain', dict(self.env, HGS_PARENT=inherited), new=True)
        self.assertNotIn('parent', plain)


if __name__ == "__main__":
    unittest.main()
