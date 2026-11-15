"""Archive input isolation with private state and fake-agent resume only."""
import hashlib
import json
import os
from pathlib import Path
import subprocess
import unittest
import uuid

if os.name == "posix":
    import test_pause as pause
else:
    pause = None


@unittest.skipUnless(os.name == "posix" and pause.TMUX, "requires private Unix tmux")
class ArchiveMetadataAvailability(pause.Harness if pause else unittest.TestCase):
    needle = "archive-r1-unrelated-needle"

    def available(self):
        record, path = self.saved_v1(tag="available")
        history = Path(record["transcript"])
        with history.open("a") as stream:
            stream.write(json.dumps({
                "type": "assistant", "sessionId": record["conversation_id"],
                "timestamp": "2026-01-01T10:00:00Z",
                "message": {"role": "assistant", "content": [{"type": "text", "text": self.needle}]},
            }) + "\n")
        self.search_available(record["name"])
        self.hgs("--dry-run", "resume", record["name"])
        return record, path

    def search_available(self, name):
        result = json.loads(self.hgs("search", "--query", self.needle))
        self.assertIn(name, [row["name"] for row in result["results"]], result)

    def archived(self, *, large=False):
        archive_id, conversation, session = (str(uuid.uuid4()) for _ in range(3))
        history = self.root / "archived-history.jsonl"
        history.write_text(json.dumps({"session_id": conversation}) + "\n")
        record = dict(version=1, name="claude/p/archived-other", agent="claude",
                      session_id=session, run_id=str(uuid.uuid4()), conversation_id=conversation,
                      archive_id=archive_id, archived_at=100, state="archived", created=1,
                      cwd=str(self.project), launch_dir=str(self.project), base=["claude"],
                      hgs=str(pause.HGS), shell=str(self.bin / "shell"), paused=True,
                      pid=999999999, process_start="exited synthetic process", transcript=str(history))
        if large:
            # A normal bounded job map, with each retained output below 32 KiB.
            record["shell_jobs"] = {}
            for i in range(128):
                call = f"call-{i}"
                job_id = hashlib.sha256(json.dumps([conversation, "main", call], separators=(",", ":")).encode()).hexdigest()[:32]
                record["shell_jobs"][job_id] = dict(
                    id=job_id, call_id=call, owner="main", provider="claude",
                    run_id=record["run_id"], conversation_id=conversation, cwd=str(self.project),
                    started_at=1, updated_at=2, status="completed", source="hooks",
                    background=True, observed=True, command=f"printf synthetic-{i}",
                    output="synthetic completed output\n" * 350, output_truncated=False,
                    capabilities=dict(output=True, stop=False, stdin=False))
        directory = self.root / "state/archives"
        directory.mkdir(exist_ok=True)
        path = directory / (archive_id + ".json")
        path.write_text(json.dumps(record))
        if large:
            self.assertGreater(path.stat().st_size, 1 << 20)
            self.assertLess(path.stat().st_size, 16 << 20)
        return record, path

    def lookup(self, record):
        result = subprocess.run([str(pause.HGS), "archives", "lookup", "--json"],
                                input=json.dumps({key: record[key] for key in ("name", "archive_id")}),
                                env=self.env, capture_output=True, text=True, timeout=20)
        self.assertEqual(result.returncode, 0, result.stderr)
        value = json.loads(result.stdout)
        self.assertEqual(value["conversation_id"], record["conversation_id"])
        self.assertEqual(value["run_id"], record["run_id"])

    def test_large_shell_job_archive_keeps_other_resume_and_search_available(self):
        available, _ = self.available()
        archived, path = self.archived(large=True)
        before = path.read_bytes()
        with self.subTest(operation="search another name"):
            self.search_available(available["name"])
        with self.subTest(operation="resume another name"):
            self.resume(available["name"])
            self.assertEqual(self.binding(available["name"])["conversation_id"], available["conversation_id"])
        with self.subTest(operation="read the valid large archive"):
            self.lookup(archived)
        self.assertEqual(path.read_bytes(), before)

    def test_unreadable_archive_keeps_reads_available_but_never_authorizes_removal(self):
        available, _ = self.available()
        archived, path = self.archived()
        before = path.read_bytes()
        mirror = {key: value for key, value in archived.items()
                  if key not in ("archive_id", "archived_at", "state")}
        mirror["paused"] = False
        mirror_path = self.root / "state" / (hashlib.sha256(mirror["name"].encode()).hexdigest() + ".json")
        mirror_path.write_text(json.dumps(mirror))
        mirror_before = mirror_path.read_bytes()
        bad = path.parent / (str(uuid.uuid4()) + ".json")
        bad.write_text("{unreadable archive")
        try:
            with self.subTest(operation="search another name"):
                self.search_available(available["name"])
            with self.subTest(operation="resume another name"):
                self.resume(available["name"])
            self.assertTrue(mirror_path.exists(), "incomplete reconciliation must retain every binding")
            with self.subTest(operation="find another requested conversation"):
                launches = self.root / "argv.jsonl"
                before_launches = launches.read_bytes() if launches.exists() else b""
                self.hgs("claude", "p", "-c", available["conversation_id"], "-d")
                self.assertEqual(launches.read_bytes(), before_launches, "reuse the exact live session")
            with self.subTest(operation="resolve another archive reference"):
                reference = json.loads(self.hgs("reference", archived["name"], "--archive", archived["archive_id"], "--json"))
                resolved = json.loads(self.hgs("inspect", reference["uri"]))
                self.assertEqual(resolved["archive_id"], archived["archive_id"])
            with self.subTest(operation="termination needs complete archive evidence"):
                self.hgs("__state", "terminate", mirror["name"], "--expected-run-id", mirror["run_id"], rc=1)
                self.assertEqual(mirror_path.read_bytes(), mirror_before)
            self.assertEqual(mirror_path.read_bytes(), mirror_before, "incomplete reconciliation must retain every binding")
            self.assertEqual(path.read_bytes(), before)
            self.assertEqual(bad.read_text(), "{unreadable archive")
        finally:
            bad.unlink()
        # The same source is admitted when the inventory is complete.
        self.hgs("__state", "terminate", mirror["name"], "--expected-run-id", mirror["run_id"])
        # A complete canonical inventory alone does not make an unfinished
        # display/identity backfill complete. Settle the private metadata helper
        # before asserting the positive crash-copy reconciliation.
        self.hgs("__state", "archive-catalog-worker")
        self.hgs("resume", available["name"], "-d")
        self.assertFalse(mirror_path.exists(), "complete positive archive evidence still reconciles the binding")
        self.assertEqual(path.read_bytes(), before)
