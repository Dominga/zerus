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
        if os.name == "nt":
            # The existing override admits __state for an isolated state only;
            # this suite never installs or starts a session host.
            self.env.update(USERPROFILE=str(self.root), HGS_HOST_ALLOW_UNSTAGED="1")

    def invoke(self, flag, request=None, check=True):
        flags = flag if isinstance(flag, list) else [flag]
        result = subprocess.run([str(HGS), "__state", "search", *flags], env=self.env,
                                input=json.dumps(request) if request is not None else "",
                                text=True, encoding="utf-8", capture_output=True, timeout=5)
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
                                       "transcript": str(transcript), "cwd": str(self.state)}))
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

    def test_catchup_is_bounded_and_reports_live_fallback(self):
        self.fixture()
        reply = self.invoke("--index-catchup")
        progress = reply["catchup"]
        self.assertEqual(progress["visited_paths"], 1)
        self.assertLessEqual(progress["ingestion"]["records"], 128)
        self.assertLessEqual(progress["ingestion"]["bytes"], 4 << 20)
        self.assertEqual(reply["status"]["catalog"]["fallback_sources"], 1)
        self.assertNotEqual(reply["status"]["coverage"]["state"], "ready")

    def indexed(self, query="needle", limit=2, cursor=None, check=True):
        args = ["--indexed", "--query", query, "--limit", str(limit)]
        if cursor is not None:
            args += ["--cursor", cursor]
        return self.invoke(args, check=check)

    def archive_fixture(self):
        identity, binding, transcript = self.fixture()
        record = json.loads(binding.read_text())
        archive_id = str(uuid.uuid4())
        record.update(archive_id=archive_id, archived_at=42)
        archives = self.state / "archives"
        archives.mkdir()
        archived = archives / (archive_id + ".json")
        archived.write_text(json.dumps(record))
        binding.unlink()
        return identity, archived, transcript

    def warm_source(self, messages=1):
        # Warm-query contracts require a caught-up source. Each subprocess keeps
        # the production budgets; slow disks resume the persisted cursor.
        for _ in range(12):
            reply = self.invoke("--index-catchup")
            coverage = reply["status"]["coverage"]
            if coverage["messages"] == messages and coverage["pending_sources"] == 0:
                return
        self.fail("source did not catch up within 12 bounded passes: " + json.dumps(reply))

    def test_indexed_archive_pages_use_exact_targets_and_reject_mismatched_cursors(self):
        identity, archived, transcript = self.archive_fixture()
        events = [dict(type="event_msg", timestamp=i, payload=dict(type="agent_message",
                  id="message-" + str(i), message="needle response " + str(i))) for i in range(1, 5)]
        transcript.write_text("".join(json.dumps(event) + "\n" for event in events))
        self.warm_source(messages=4)
        reply = self.indexed()
        self.assertEqual(reply["abi"], "content-search-query-v1")
        self.assertEqual(reply["index_coverage"]["state"], "ready")
        self.assertIsNone(reply["fallback"])
        self.assertEqual([r["message_id"] for r in reply["results"]], ["message-4", "message-3"])
        target = reply["results"][0]
        self.assertEqual(target["run_id"], identity["run_id"])
        self.assertEqual(target["session"]["archive_id"], archived.stem)
        second = self.indexed(cursor=reply["next_cursor"])
        self.assertEqual([r["message_id"] for r in second["results"]], ["message-2", "message-1"])
        self.assertIsNone(second["next_cursor"])
        self.assertNotEqual(self.indexed(query="other", cursor=reply["next_cursor"], check=False).returncode, 0)
        # A forgotten target cannot leak through a continuation between passes.
        archived.unlink()
        forgotten = self.indexed(cursor=reply["next_cursor"])
        self.assertEqual(forgotten["results"], [])

    def test_indexed_live_queries_keep_the_current_bounded_scan_and_new_messages(self):
        _, _, transcript = self.fixture()
        self.warm_source()
        first = self.indexed()
        self.assertEqual(first["results"][0]["message_id"], "public-one")
        self.assertIsNotNone(first["fallback"])
        self.assertEqual(first["fallback"]["results"][0]["message_id"], "public-one")
        with transcript.open("a") as stream:
            stream.write(json.dumps(dict(type="event_msg", timestamp=43,
                         payload=dict(type="agent_message", id="new-turn", message="needle new turn"))) + "\n")
        second = self.indexed()
        self.assertEqual(second["fallback"]["results"][0]["message_id"], "new-turn")
        self.assertEqual(second["results"], [])
        self.warm_source(messages=2)
        third = self.indexed()
        self.assertEqual(third["results"][0]["message_id"], "new-turn")
        self.assertEqual(third["fallback"]["results"][0]["message_id"], "new-turn")

    def test_indexed_kimi_uses_the_exact_unique_public_wire_without_an_engine(self):
        identity, binding, _ = self.fixture()
        binding.unlink()
        identity["name"] = "kimi/example/search"
        profile = self.root / "kimi-profile"
        workspace = profile / "sessions" / "workspace-a"
        wire = workspace / identity["conversation_id"] / "agents" / "main" / "wire.jsonl"
        wire.parent.mkdir(parents=True)
        (profile / "sessions" / "workspace-b").mkdir()
        event = dict(type="agent.message.appended", agentId="main", time=42000,
                     message=dict(meta=dict(messageId="kimi-public"), message=dict(role="assistant",
                                  content=[dict(type="think", think="PRIVATE_REASONING_BODY"),
                                           dict(type="text", text="needle kimi public message")])))
        wire.write_text(json.dumps(event) + "\n")
        binding = self.state / (hashlib.sha256(identity["name"].encode()).hexdigest() + ".json")
        binding.write_text(json.dumps({**identity, "version": 1, "agent": "kimi", "agent_home": str(profile), "cwd": str(self.state)}))
        before = wire.read_bytes()
        self.warm_source()
        reply = self.indexed()
        self.assertEqual(reply["results"][0]["message_id"], "kimi-public")
        self.assertEqual(reply["fallback"]["results"][0]["message_id"], "kimi-public")
        self.assertLessEqual(reply["proof"]["directory_entries"], 2000)
        self.assertEqual(wire.read_bytes(), before)
        raw = (self.state / "content-search-v1" / "index.sqlite3").read_bytes()
        self.assertNotIn(b"PRIVATE_REASONING_BODY", raw)
        # Adding a duplicate under an existing workspace changes only its child stamp.
        duplicate = profile / "sessions" / "workspace-b" / identity["conversation_id"] / "agents" / "main" / "wire.jsonl"
        duplicate.parent.mkdir(parents=True)
        duplicate.write_bytes(before)
        unavailable = self.indexed()
        self.assertEqual(unavailable["results"], [])
        self.assertEqual(unavailable["fallback"]["results"], [])
        self.assertGreater(unavailable["fallback"]["unavailable_sessions"], 0)

    @unittest.skipIf(os.name == "nt", "Unix private file permissions")
    def test_an_unsafe_copied_index_is_refused_and_uses_legacy_fallback(self):
        self.fixture()
        self.invoke("--index-catchup")
        index = self.state / "content-search-v1" / "index.sqlite3"
        index.chmod(0o644)
        reply = self.indexed()
        self.assertEqual(reply["index_coverage"]["state"], "unavailable")
        self.assertEqual(reply["fallback"]["results"][0]["message_id"], "public-one")
        self.assertEqual(index.stat().st_mode & 0o777, 0o644)

    def test_discovery_queues_paths_without_reading_or_claiming_indexed_content(self):
        _, binding, _ = self.fixture()
        binding.write_text("PRIVATE_BINDING_BODY_NEVER_STAGED")
        reply = self.invoke("--index-discover")
        self.assertTrue(reply["discovery"]["complete"])
        self.assertEqual(reply["status"]["catalog"]["queued_paths"], 1)
        self.assertEqual(reply["status"]["catalog"]["pending_paths"], 1)
        self.assertEqual(reply["status"]["coverage"]["messages"], 0)
        raw = (self.state / "content-search-v1" / "index.sqlite3").read_bytes()
        self.assertNotIn(b"PRIVATE_BINDING_BODY_NEVER_STAGED", raw)


if __name__ == "__main__":
    unittest.main(verbosity=2)
