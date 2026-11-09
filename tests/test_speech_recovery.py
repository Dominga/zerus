"""Real original callers and receipts across owned process exit, with synthetic audio."""
from contextlib import closing
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
from pathlib import Path
import sqlite3
import socketserver
import subprocess
import sys
import tempfile
import threading
import time
import unittest
import uuid

REPO = Path(__file__).resolve().parents[1]
HGS = Path(os.environ.get("HGS_TEST_BIN", REPO / "target/debug/hgs")).resolve()
KEY = "synthetic-recovery-key"


@unittest.skipUnless(os.name == "posix", "private speech authorities require Unix")
class SpeechRecovery(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        selected = os.environ.get("HGS_TEST_SPEECH_RECOVERY_BIN")
        if selected is not None:
            cls.fixture = Path(selected)
            if not selected or not cls.fixture.is_file() or not os.access(cls.fixture, os.X_OK):
                raise AssertionError("The prepared speech recovery fixture is not executable")
            cls.fixture = cls.fixture.resolve()
            return
        build = subprocess.run(["cargo", "+1.99.1", "test", "--locked", "--test",
                                "voice_recovery_fixture", "--no-run", "--message-format=json"],
                               cwd=REPO, capture_output=True, text=True, timeout=240)
        if build.returncode:
            raise AssertionError(build.stderr[-4000:])
        artifacts = [json.loads(line) for line in build.stdout.splitlines() if line.startswith("{")]
        cls.fixture = next(Path(a["executable"]) for a in artifacts
                           if a.get("reason") == "compiler-artifact" and a.get("executable")
                           and a["target"]["name"] == "voice_recovery_fixture")

    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="zsr-", dir=os.environ.get("ZERUS_TEST_SHORT_TMPDIR", "/tmp"))
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.config = self.root / "config"
        self.config.mkdir(mode=0o700)
        self.client = self.root / "client"
        self.client.mkdir(mode=0o700)
        self.path = self.config / "speech.json"
        self.path.write_text(json.dumps({"schema": 1, "profiles": [],
            "state_dir": str(self.root / "speech"), "socket_path": str(self.root / "speech.sock")}))
        self.env = {k: v for k, v in os.environ.items()
                    if not k.startswith(("HGS_", "ZERUS_"))}
        self.env.update(HOME=str(self.root), HGS_CONFIG_DIR=str(self.config),
                        HGS_STATE_DIR=str(self.root / "sessions"),
                        XDG_STATE_HOME=str(self.root / "state"),
                        XDG_CONFIG_HOME=str(self.root / "xdg"),
                        PYTHONPATH=str(REPO / "services/speech"))
        self.children = []
        self.addCleanup(self.stop_children)
        self.entered = threading.Event()
        self.release = threading.Event()
        self.requests = []
        owner = self

        class Handler(BaseHTTPRequestHandler):
            def do_POST(self):
                self.rfile.read(int(self.headers.get("Content-Length", "0")))
                owner.requests.append((self.path, self.headers.get("Authorization")))
                owner.entered.set()
                if not owner.release.wait(25):
                    return
                body = b'{"text":"Synthetic recovery transcript"}'
                try:
                    self.send_response(200)
                    self.send_header("Content-Length", str(len(body)))
                    self.end_headers()
                    self.wfile.write(body)
                except OSError:
                    pass

            def log_message(self, *_args):
                pass

        self.http = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        self.thread = threading.Thread(target=self.http.serve_forever, daemon=True)
        self.thread.start()
        self.addCleanup(self.stop_http)
        self.initial = self.cli({"action": "config_list"})
        self.machine = self.initial["machine_id"]
        self.saved = self.cli(self.upsert(self.initial["revision"]))
        self.assertEqual(self.saved["state"], "completed")
        self.service = self.start_service()

    def stop_http(self):
        self.release.set()
        self.http.shutdown()
        self.http.server_close()
        self.thread.join(timeout=3)

    def stop_children(self):
        for child in reversed(self.children):
            if child.poll() is None:
                child.kill()  # Only a Popen created and retained by this fixture.
            child.wait(timeout=5)
            if child.stderr is not None:
                child.stderr.close()

    def upsert(self, revision, *, changed=False):
        url = "http://127.0.0.1:1/v1" if changed else "http://127.0.0.1:%d/v1" % self.http.server_port
        return {"action": "profile_upsert", "request_id": str(uuid.uuid4()),
            "expected_machine_id": self.machine, "expected_revision": revision,
            "profile": {"id": "remote", "name": "Recovery fixture", "adapter": "openai_http",
                "model": "fixture", "language": "en", "base_url": url, "allow_insecure_http": True},
            "credential": {"mode": "replace", "value": "synthetic-new-key" if changed else KEY}}

    def cli(self, payload):
        result = subprocess.run([str(HGS), "speech", "--json"], input=json.dumps(payload),
                                env=self.env, text=True, capture_output=True, timeout=15)
        self.assertEqual(result.returncode, 0, result.stderr[-1000:])
        return json.loads(result.stdout)

    def start_service(self):
        child = subprocess.Popen([sys.executable, "-m", "zerus_speech", "--config", str(self.path)],
            env=self.env, stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        self.children.append(child)
        deadline = time.monotonic() + 8
        while time.monotonic() < deadline:
            self.assertIsNone(child.poll(), "private speech service exited")
            if (self.root / "speech.sock").exists():
                # Empty/removed profiles do not make a live receipt authority
                # unavailable. Wait for an actual stats RPC, not discovery.
                result = subprocess.run([str(HGS), "speech", "--json"],
                    input='{"action":"stats"}', env=self.env, text=True,
                    capture_output=True, timeout=3)
                if result.returncode == 0 and "totals" in json.loads(result.stdout):
                    return child
            time.sleep(0.02)
        self.fail("private speech service readiness deadline")

    def caller(self, kind, *, fresh, action="profile_test"):
        token = uuid.uuid4().hex
        control = self.root / (token + ".json")
        result = self.root / (token + ".result.json")
        control.write_text(json.dumps({"kind": kind, "fresh": fresh, "action": action, "machine": self.machine,
            "client": str(self.client), "hgs": str(HGS), "result": str(result)}))
        env = dict(self.env, ZERUS_TEST_SPEECH_RECOVERY="1", ZERUS_TEST_SPEECH_CONTROL=str(control))
        child = subprocess.Popen([str(self.fixture), "--exact", "client_fixture", "--ignored", "--nocapture"],
            env=env, stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)
        self.children.append(child)
        deadline = time.monotonic() + 12
        while not result.exists() and time.monotonic() < deadline:
            if child.poll() is not None:
                self.fail("client fixture exited before its durable result: " + child.stderr.read(2000).decode(errors="replace"))
            time.sleep(0.01)
        self.assertTrue(result.exists(), "bounded caller result deadline")
        data = json.loads(result.read_text())
        self.assertNotIn(KEY, result.read_text())
        if not fresh:
            self.assertEqual(child.wait(timeout=3), 0)
        return child, data

    def settle(self, payload, state):
        deadline = time.monotonic() + 8
        while time.monotonic() < deadline:
            result = self.cli(payload)
            if result.get("state") == state:
                return result
            time.sleep(0.02)
        self.fail("original receipt did not settle: " + str(result.get("state")))

    def accepted_then_kill(self, kind):
        child, first = self.caller(kind, fresh=True)
        self.assertTrue(self.entered.wait(3), "original synthetic inference was not accepted")
        binding = first["after"]
        if kind == "voice":
            self.capture_sha256 = first["audio_sha256"]
            self.assertEqual(len(self.capture_sha256), 64)
        self.assertEqual(binding["speech"]["profile_revision"] if kind == "voice"
                         else binding["expected_profile_revision"], self.saved["profile_revision"])
        child.kill()
        child.wait(timeout=3)
        self.assertLess(child.returncode, 0, "the original caller must really be killed")
        self.assertEqual(len(self.requests), 1)
        return binding

    def assert_read_only(self, data, kind, binding):
        actions = [c["action"] for c in data["calls"]]
        forbidden = {"draft_submit", "submit", "profile_test", "profile_upsert", "profile_remove"}
        self.assertTrue(forbidden.isdisjoint(actions), actions)
        status = next(c for c in data["calls"] if c["action"] == ("draft_status" if kind == "voice" else "config_status"))
        self.assertEqual(status["job_id"] if kind == "voice" else status["operation_id"],
                         binding["id"] if kind == "voice" else binding["request_id"])
        self.assertEqual(status["machine_id"] if kind == "voice" else status["expected_machine_id"], self.machine)

    def test_voice_client_kill_and_revoked_editor_keep_original_completion_reviewable(self):
        original = self.accepted_then_kill("voice")
        self.assertEqual(original["draft_revision"], 7)
        self.assertEqual(original["operation_id"], original["id"])
        self.release.set()
        self.settle({"action": "draft_status", "machine_id": self.machine, "job_id": original["id"]}, "completed")
        _, recovered = self.caller("voice", fresh=False)
        self.assert_read_only(recovered, "voice", original)
        for key in ["id", "operation_id", "speech", "draft_revision", "run", "conversation"]:
            self.assertEqual(recovered["after"][key], original[key])
        self.assertEqual(recovered["after"]["transcript"], "Synthetic recovery transcript")
        self.assertEqual(recovered["audio_sha256"], self.capture_sha256)
        self.assertEqual(len(self.requests), 1)

    def test_voice_service_kill_restart_preserves_unknown_without_audio_replay(self):
        original = self.accepted_then_kill("voice")
        self.service.kill(); self.service.wait(timeout=3)
        self.release.set()
        self.service = self.start_service()
        for _ in range(2):
            _, recovered = self.caller("voice", fresh=False)
            self.assert_read_only(recovered, "voice", original)
            self.assertEqual(recovered["after"], original)
            self.assertEqual(recovered["state"], "uncertain")
            self.assertEqual(recovered["audio_sha256"], self.capture_sha256)
        self.assertEqual(len(self.requests), 1)

    def test_provider_client_kill_and_new_revision_use_only_original_probe_receipt(self):
        original = self.accepted_then_kill("provider")
        newer = self.cli(self.upsert(self.saved["revision"], changed=True))
        self.assertGreater(newer["profile_revision"], original["expected_profile_revision"])
        self.release.set()
        status = {"action": "config_status", "expected_machine_id": self.machine,
                  "operation_id": original["request_id"]}
        receipt = self.settle(status, "completed")
        self.assertEqual(receipt["profile_revision"], original["expected_profile_revision"])
        _, recovered = self.caller("provider", fresh=False)
        self.assertEqual(recovered["before"], original)
        self.assert_read_only(recovered, "provider", original)
        self.assertFalse(recovered["pending_visible"])
        self.assertEqual(self.requests, [("/v1/audio/transcriptions", "Bearer " + KEY)])
        self.assertNotIn(KEY, (self.client / "settings.json").read_text())

    def test_provider_service_kill_restart_keeps_original_operation_unknown(self):
        original = self.accepted_then_kill("provider")
        self.service.kill(); self.service.wait(timeout=3)
        self.release.set()
        self.service = self.start_service()
        for _ in range(2):
            _, recovered = self.caller("provider", fresh=False)
            self.assert_read_only(recovered, "provider", original)
            self.assertEqual(recovered["before"], original)
            self.assertEqual(recovered["after"], original)
            self.assertTrue(recovered["pending_visible"])
        self.assertEqual(len(self.requests), 1)
        with closing(sqlite3.connect(self.config / "speech-profiles/profiles.sqlite3")) as db:
            result = json.loads(db.execute("SELECT result FROM operations WHERE id=?",
                                         [original["request_id"]]).fetchone()[0])
        self.assertEqual(result["state"], "uncertain")
        self.assertEqual(result["error_code"], "service_restarted")

    def test_provider_receipt_cannot_settle_from_another_job_or_schema(self):
        original = self.accepted_then_kill("provider")
        self.service.kill(); self.service.wait(timeout=3)
        self.release.set()
        path = self.root / "speech.sock"
        path.unlink()  # Its exact fixture owner has exited and was reaped.
        wrong = {"schema": 1, "job_id": str(uuid.uuid4()), "state": "completed"}
        observed = []

        class Handler(socketserver.StreamRequestHandler):
            def handle(self):
                request = json.loads(self.rfile.readline(4096))
                observed.append(request)
                reply = wrong if request["action"] == "status" else {"schema": 1, "available": False, "profiles": []}
                self.wfile.write(json.dumps(reply).encode() + b"\n")

        server = socketserver.UnixStreamServer(str(path), Handler)
        path.chmod(0o600)
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()
        self.addCleanup(lambda: (server.shutdown(), server.server_close(), thread.join(timeout=3)))
        # Neither a different job's completion nor a future-schema reply for
        # our ID can settle the original provider operation.
        for invalid in [{"schema": 1, "job_id": str(uuid.uuid4()), "state": "completed"},
                        {"schema": 99, "job_id": original["request_id"], "state": "completed"}]:
            wrong.clear(); wrong.update(invalid)
            _, recovered = self.caller("provider", fresh=False)
            self.assert_read_only(recovered, "provider", original)
            self.assertEqual(recovered["after"], original)
            self.assertTrue(recovered["pending_visible"])
        statuses = [r for r in observed if r["action"] == "status"]
        self.assertEqual(len(statuses), 2)
        self.assertTrue(all(r["job_id"] == original["request_id"] for r in statuses))
        self.assertEqual(len(self.requests), 1)

    def test_legacy_uncertain_receipt_stays_reviewable_with_service_offline(self):
        original = self.accepted_then_kill("provider")
        self.service.kill(); self.service.wait(timeout=3)
        self.release.set()
        path = self.config / "speech-profiles/profiles.sqlite3"
        with closing(sqlite3.connect(path)) as db:
            receipt = json.loads(db.execute("SELECT result FROM operations WHERE id=?",
                                           [original["request_id"]]).fetchone()[0])
        # Reconstruct only the old reader's misclassification; the real
        # original request, profile and receipt bindings remain unchanged.
        for code in ["service_restarted", "result_expired"]:
            legacy = dict(receipt, state="failed", error_code=code, error="Legacy unknown result")
            with closing(sqlite3.connect(path)) as db:
                db.execute("UPDATE operations SET result=? WHERE id=?",
                           [json.dumps(legacy), original["request_id"]])
                db.commit()
            _, recovered = self.caller("provider", fresh=False)
            self.assert_read_only(recovered, "provider", original)
            self.assertEqual(recovered["after"], original)
            self.assertTrue(recovered["pending_visible"])
            with closing(sqlite3.connect(path)) as db:
                saved = json.loads(db.execute("SELECT result FROM operations WHERE id=?",
                                             [original["request_id"]]).fetchone()[0])
            self.assertEqual(saved["state"], "uncertain")
            self.assertEqual(saved["error_code"], code)
        self.assertEqual(len(self.requests), 1)

    def test_provider_edit_and_remove_survive_lost_reply_and_service_restart(self):
        path = self.config / "speech-profiles/profiles.sqlite3"
        for index, action in enumerate(["profile_upsert", "profile_remove"]):
            with self.subTest(action=action):
                revision = self.cli({"action": "config_list"})["revision"]
                child, first = self.caller("provider", fresh=True, action=action)
                original = first["after"]
                self.assertEqual(original["expected_revision"], revision)
                self.assertEqual(original["profile_id"], "remote")
                child.kill(); child.wait(timeout=3)
                self.assertLess(child.returncode, 0)
                self.service.kill(); self.service.wait(timeout=3)
                self.service = self.start_service()
                _, recovered = self.caller("provider", fresh=False)
                self.assertEqual(recovered["before"], original)
                self.assert_read_only(recovered, "provider", original)
                self.assertFalse(recovered["pending_visible"])
                receipt = self.cli({"action": "config_status", "expected_machine_id": self.machine,
                                    "operation_id": original["request_id"]})
                self.assertEqual(receipt["state"], "completed")
                self.assertEqual(receipt["revision"], revision + 1)
                with closing(sqlite3.connect(path)) as db:
                    self.assertEqual(db.execute("SELECT count(*) FROM operations").fetchone()[0], index + 2)
                self.assertNotIn(KEY, (self.client / "settings.json").read_text())
                self.assertEqual(self.requests, [], "editing/removing must not submit audio")
