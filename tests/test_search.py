#!/usr/bin/env python3
"""Read-only content search, isolated bindings/transcripts and no agent processes."""
from contextlib import closing
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


class ContentSearch(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="hgs-search-")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.state = self.root / "state"
        self.state.mkdir()
        self.env = dict(os.environ, HOME=str(self.root), HGS_CONFIG_DIR=str(self.root / "config"),
                        HGS_STATE_DIR=str(self.state), HGS_SELF="fixture", HGS_PEERS="")
        for key in ("TMUX", "TMUX_PANE", "HGS_RUN_ID", "HGS_SESSION"):
            self.env.pop(key, None)
        if os.name == "nt":
            # Admit __state only for this explicitly isolated fixture. No host
            # is installed or started, and the real Windows profile stays out.
            self.env.update(USERPROFILE=str(self.root), HGS_HOST_ALLOW_UNSTAGED="1")

    def binding(self, name, text, archive=False, agent="codex"):
        sid = str(uuid.uuid4())
        path = self.root / (sid + ".jsonl")
        path.write_text(json.dumps({"type": "event_msg", "timestamp": "2026-01-01T10:00:00Z",
                                   "payload": {"type": "agent_message", "message": text}}, ensure_ascii=False) + "\n", encoding="utf-8")
        record = dict(version=1, name=name, agent=agent, cwd="/work/project", run_id=str(uuid.uuid4()),
                      conversation_id=sid, transcript=str(path), updated=42)
        if archive:
            record["archive_id"] = str(uuid.uuid4())
            folder = self.state / "archives"
            folder.mkdir(exist_ok=True)
            destination = folder / (record["archive_id"] + ".json")
        else:
            destination = self.state / (hashlib.sha256(name.encode()).hexdigest() + ".json")
        destination.write_text(json.dumps(record))
        return record, destination

    def run_search(self, *args, check=True):
        result = subprocess.run([str(HGS), "__state", "search", *args], env=self.env,
                                capture_output=True, text=True, encoding="utf-8", timeout=8)
        if check:
            self.assertEqual(result.returncode, 0, result.stderr)
            return json.loads(result.stdout)
        return result

    def snapshot(self):
        return {str(path.relative_to(self.root)): path.read_bytes()
                for path in self.root.rglob("*") if path.is_file()}

    def test_same_name_archive_and_active_are_distinct_and_read_only(self):
        active, _ = self.binding("codex/project/task", "Current МИГРАЦИЯ")
        archived, _ = self.binding("codex/project/task", "Earlier миграция", archive=True)
        # Malformed obsolete bindings are reported by the loader, not fatal to search.
        (self.state / "corrupt.json").write_text("not JSON")
        before = self.snapshot()
        result = self.run_search("--query", "миграция")
        self.assertEqual(len(result["results"]), 2)
        self.assertEqual({hit["archive_id"] for hit in result["results"]}, {"", archived["archive_id"]})
        self.assertEqual({hit["conversation_id"] for hit in result["results"]},
                         {active["conversation_id"], archived["conversation_id"]})
        self.assertEqual(before, self.snapshot())

    def test_journal_and_provider_duplicate_are_one_public_message(self):
        record, _ = self.binding("codex/project", "Резервная копия успешно завершена")
        with closing(sqlite3.connect(self.state / "events.sqlite3")) as db, db:
            db.execute("CREATE TABLE events (seq INTEGER PRIMARY KEY, name TEXT, conversation TEXT, payload TEXT)")
            payload = dict(type="Stop", at=1767261601, detail="Резервная копия успешно…", agent_id="")
            db.execute("INSERT INTO events(name,conversation,payload) VALUES (?,?,?)",
                       (record["name"], record["conversation_id"], json.dumps(payload)))
        result = self.run_search("--query", "копия")
        self.assertEqual(len(result["results"]), 1)
        self.assertEqual(result["results"][0]["content"], "Резервная копия успешно завершена")
        self.assertEqual(result["results"][0]["source"], "codex_transcript")

    def test_archive_hits_carry_recorded_accounts_and_exact_folder_scopes_without_writes(self):
        repo = self.root / "repo"
        subprocess.run(["git", "init", "--quiet", str(repo)], check=True, env=self.env)
        folder = repo / "subfolder"
        folder.mkdir()
        record, path = self.binding("codex/example/old", "needle public history", archive=True)
        record.update(account_id="recorded-work", agent_home=str(self.root / "old-profile"), cwd=str(folder))
        path.write_text(json.dumps(record))
        before = self.snapshot()
        session = self.run_search("--query", "needle")["results"][0]["session"]
        self.assertEqual(session["account_id"], "recorded-work")
        self.assertEqual(session["account_home"], record["agent_home"])
        self.assertEqual(Path(session["cwd_canonical"]), folder.resolve())
        self.assertEqual(Path(session["git_root"]), repo.resolve())
        self.assertEqual(before, self.snapshot())

    def test_missing_transcript_keeps_snapshot_and_marks_partial_coverage(self):
        record, binding = self.binding("codex/project", "unrelated")
        Path(record["transcript"]).unlink()
        record.update(last_message="Remember this missing history", last_event_at=42)
        binding.write_text(json.dumps(record))
        result = self.run_search("--query", "missing history")
        self.assertEqual(result["unavailable_sessions"], 1)
        self.assertEqual(result["results"][0]["source"], "snapshot")
        self.assertEqual(result["results"][0]["at"], 42)

    def test_kimi_bound_main_history_only(self):
        record, binding = self.binding("kimi/project", "unused", agent="kimi")
        kimi = self.root / "kimi"
        record["agent_home"] = str(kimi)
        binding.write_text(json.dumps(record))
        wire = kimi / "sessions" / "workspace" / record["conversation_id"] / "agents" / "main" / "wire.jsonl"
        wire.parent.mkdir(parents=True)
        events = [dict(type="agent.message.appended", agentId=agent, time=1767261600000,
                       message=dict(message=dict(role="assistant", content=[
                           dict(type="think", think="PRIVATE REASONING TOKEN"), dict(type="text", text=text)]),
                           meta=dict(messageId=str(uuid.uuid4()))))
                  for agent, text in [("main", "PUBLIC SEARCHABLE RESPONSE"), ("child", "CHILD PRIVATE TOKEN")]]
        wire.write_text("\n".join(map(json.dumps, events)))
        result = self.run_search("--query", "searchable")
        self.assertEqual(len(result["results"]), 1)
        self.assertEqual(result["results"][0]["source"], "kimi_wire")
        self.assertEqual(self.run_search("--query", "PRIVATE")["results"], [])

    def test_rejects_invalid_queries_and_limits(self):
        for args in [[], ["--query", ""], ["--query", " "], ["--query", "x\ny"],
                     ["--query", "x" * 257], ["--query", "x", "--limit", "0"],
                     ["--query", "x", "--limit", "201"], ["--query", "x", "--limit", "oops"],
                     ["--query", "x", "--unknown", "y"], ["--query", "x", "--query", "y"]]:
            with self.subTest(args=args):
                result = self.run_search(*args, check=False)
                self.assertNotEqual(result.returncode, 0)
                self.assertTrue(result.stderr)
        self.assertEqual(self.snapshot(), {})


if __name__ == "__main__":
    unittest.main()
