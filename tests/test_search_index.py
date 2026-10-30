#!/usr/bin/env python3
"""Private on-demand index protocol; no agents, desktop, services or live data."""
import hashlib
import json
import os
from pathlib import Path
import sqlite3
import subprocess
import tempfile
import unittest
import uuid

REPO = Path(__file__).resolve().parents[1]
HGS = Path(os.environ.get("HGS_TEST_BIN", REPO / "target/debug/hgs")).resolve()


class SearchIndex(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="hgs-index-")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.state = self.root / "state"
        self.state.mkdir(mode=0o700)
        self.env = {key: value for key, value in os.environ.items()
                    if not key.startswith(("HGS_", "ZERUS_CLIENT_")) and key not in ("TMUX", "TMUX_PANE")}
        self.env.update(HOME=str(self.root), HGS_STATE_DIR=str(self.state),
                        HGS_CONFIG_DIR=str(self.root / "config"), HGS_SELF="fixture", HGS_PEERS="")

    def invoke(self, flag, request=None, check=True):
        result = subprocess.run([str(HGS), "__state", "search", flag], env=self.env,
                                input=json.dumps(request) if request is not None else "",
                                text=True, capture_output=True, timeout=5)
        if check:
            self.assertEqual(result.returncode, 0, result.stderr)
            return json.loads(result.stdout)
        return result

    def fixture(self):
        identity = dict(name="codex/example/search", run_id=str(uuid.uuid4()),
                        conversation_id=str(uuid.uuid4()), archive_id="")
        transcript = self.root / (identity["conversation_id"] + ".jsonl")
        event = dict(type="event_msg", timestamp=42,
                     payload=dict(type="agent_message", id="public-one", message="needle public response"))
        transcript.write_text(json.dumps(event) + "\n")
        binding = self.state / (hashlib.sha256(identity["name"].encode()).hexdigest() + ".json")
        binding.write_text(json.dumps({**identity, "version": 1, "agent": "codex",
                                       "transcript": str(transcript), "cwd": "/work/example"}))
        return identity, binding, transcript

    def test_cold_status_does_not_create_an_index(self):
        before = list(self.state.iterdir())
        reply = self.invoke("--index-status")
        self.assertEqual(reply["coverage"]["state"], "cold")
        self.assertEqual(list(self.state.iterdir()), before)

    def test_step_uses_exact_target_and_preserves_authoritative_files(self):
        identity, binding, transcript = self.fixture()
        before = binding.read_bytes(), transcript.read_bytes()
        reply = self.invoke("--index-step", dict(abi="content-search-index-v1", targets=[identity]))
        self.assertEqual(reply["abi"], "content-search-index-v1")
        self.assertEqual(reply["progress"]["targets"], 1)
        self.assertLessEqual(reply["progress"]["records"], 128)
        self.assertLessEqual(reply["progress"]["bytes"], 4 << 20)
        self.assertEqual(reply["status"]["coverage"]["messages"], 1)
        self.assertEqual((binding.read_bytes(), transcript.read_bytes()), before)
        index = self.state / "content-search-v1" / "index.sqlite3"
        if os.name != "nt":
            self.assertEqual(index.stat().st_mode & 0o777, 0o600)
            self.assertEqual(index.parent.stat().st_mode & 0o777, 0o700)
        # The next pass catches a forgotten source even when it is not requested.
        binding.unlink()
        self.invoke("--index-step", dict(abi="content-search-index-v1", targets=[]))
        with sqlite3.connect(index) as db:
            self.assertEqual(db.execute("SELECT COUNT(*) FROM messages").fetchone()[0], 0)
            self.assertEqual(db.execute("SELECT SUM(active) FROM sources").fetchone()[0], 0)

    def test_protocol_rejects_wrong_abi_and_unbounded_target_lists(self):
        identity, _, _ = self.fixture()
        for request in [dict(abi="unknown", targets=[identity]),
                        dict(abi="content-search-index-v1", targets=[identity] * 17),
                        dict(abi="content-search-index-v1", targets=[{**identity, "extra": "rejected"}])]:
            self.assertNotEqual(self.invoke("--index-step", request, check=False).returncode, 0)


if __name__ == "__main__":
    unittest.main(verbosity=2)
