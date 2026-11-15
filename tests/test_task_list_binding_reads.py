"""Task pages must not read unrelated saved session bodies (zerus-bdqbb)."""
import base64
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import time
import unittest
import uuid

from test_tasks import Fixture, HGS, agent_record
from test_watch import Stream


@unittest.skipUnless(sys.platform == "linux", "Linux child I/O accounting")
class TaskListBindingReads(Fixture):
    def setUp(self):
        super().setUp()
        # Prove this worker's own PID/birth as a private fixture agent. The
        # real ancestor guard stays enabled; no Person authority is fabricated.
        name = "codex/private-page-reader"
        self.env.update(HGS_SESSION=name, HGS_RUN_ID=self.agent(name), HGS_AGENT="codex")

    def add(self, text, *args, **kwargs):
        return super().add(text, *args, session="codex/private-page-reader", **kwargs)

    def unrelated_records(self):
        for n in range(200):
            name = f"codex/unrelated/{n}"
            path = self.state / (hashlib.sha256(name.encode()).hexdigest() + ".json")
            path.write_text(json.dumps(dict(version=1, name=name, run_id=f"run-{n}",
                                           shell_jobs={"unknown": {"status": "unknown",
                                                                   "output": "x" * 65536}})))

    def measured(self, *args, **extra_env):
        """Read this child's final I/O before reaping; no host counters/cache drops."""
        with (self.root / "stdout").open("w+") as out, (self.root / "stderr").open("w+") as err:
            child = subprocess.Popen([str(HGS), "task", *args], env=dict(self.env, **extra_env),
                                     stdin=subprocess.DEVNULL, stdout=out, stderr=err)
            try:
                until = time.monotonic() + 30
                while not os.waitid(os.P_PID, child.pid, os.WEXITED | os.WNOHANG | os.WNOWAIT):
                    if time.monotonic() >= until:
                        self.fail("private task read exceeded 30 seconds")
                    time.sleep(.005)
                counts = dict(line.split(":", 1) for line in Path(f"/proc/{child.pid}/io").read_text().splitlines())
                code = child.wait()
                out.seek(0)
                err.seek(0)
                return code, out.read(), err.read(), int(counts["rchar"])
            finally:
                if child.poll() is None:
                    child.kill()
                    child.wait()

    def test_unassigned_page_reads_no_unrelated_bindings_and_polls_unchanged(self):
        for n in range(4):
            self.add(f"Task {n}")
        expected = self.hgs("list", "--project", "orchard")["tasks"]
        self.unrelated_records()
        args = ("list", "--project", "orchard", "--stage", "todo", "--limit", "2")
        code, stdout, stderr, read = self.measured("list", "--project", "orchard", "--stage", "done", "--limit", "2")
        self.assertEqual(code, 0, stderr)
        self.assertEqual(json.loads(stdout)["tasks"], [])
        self.assertLess(read, 2 * 1024 * 1024, f"an empty page read {read} bytes of unrelated state")
        # The author is this proved fixture agent. Build its routing metadata
        # once, then verify a warm page reads the small index rather than jobs.
        self.hgs(*args)
        code, stdout, stderr, read = self.measured(*args)
        self.assertEqual(code, 0, stderr)
        page = json.loads(stdout)
        self.assertEqual(page["tasks"], expected[:2])
        self.assertLess(read, 2 * 1024 * 1024, f"a two-row page read {read} bytes of unrelated state")
        code, stdout, stderr, read = self.measured(*args, "--if-revision", page["revision"])
        self.assertEqual(code, 0, stderr)
        self.assertTrue(json.loads(stdout)["unchanged"])
        self.assertLess(read, 2 * 1024 * 1024)

    def test_invalid_caller_is_refused_before_binding_reads(self):
        self.unrelated_records()
        code, _, stderr, read = self.measured("list", "--project", "orchard", "--agent", "foreign")
        self.assertNotEqual(code, 0)
        self.assertIn("only send and cancel take --agent", stderr)
        self.assertLess(read, 2 * 1024 * 1024, f"a refused caller read {read} bytes of unrelated state")

    def test_media_metadata_and_legacy_rows_stay_equal_after_routing_cache_warmup(self):
        data = b"synthetic attachment bytes"
        file = dict(name="example.png", mime="image/png", reference="[Image #1]",
                    data_base64=base64.b64encode(data).decode())
        self.add(json.dumps(dict(text="Read [Image #1]", attachments=[file])), "--content-json")
        args = ("list", "--project", "orchard", "--stage", "todo", "--limit", "1")
        first = self.hgs(*args)["tasks"]
        self.assertEqual(self.hgs(*args)["tasks"], first)
        self.assertEqual(self.hgs("list", "--project", "orchard")["tasks"], first)
        metadata = first[0]["attachments"][0]
        self.assertEqual(metadata["sha256"], hashlib.sha256(data).hexdigest())
        self.assertEqual(metadata["size"], len(data))
        self.assertNotIn("data_base64", metadata)

    def test_a_cached_page_follows_the_original_id_after_rename_and_label_reuse(self):
        launch = str(uuid.uuid4())
        name = self.env["HGS_SESSION"]
        original = self.state / (hashlib.sha256(name.encode()).hexdigest() + ".json")
        record = json.loads(original.read_text())
        record["launch_id"] = launch
        original.write_text(json.dumps(record))
        # An agent's task is pinned to itself; keep the production admission
        # rule and change only this fixture worker's own proved binding.
        task = self.add("Keep the original target")
        args = ("list", "--project", "orchard", "--stage", "todo", "--limit", "1")
        self.assertEqual(self.hgs(*args)["tasks"][0]["target"], task["target"])
        record = json.loads(original.read_text())
        original.unlink()
        record.update(name="codex/renamed", journal_name=name)
        renamed = self.state / (hashlib.sha256(record["name"].encode()).hexdigest() + ".json")
        renamed.write_text(json.dumps(record))
        self.env.update(HGS_SESSION=record["name"], HGS_RUN_ID=record["run_id"])
        agent_record(self.state, self.env, name, "replacement", launch_id=str(uuid.uuid4()))
        target = self.hgs(*args)["tasks"][0]["target"]
        self.assertEqual(target["session"], "codex/renamed")
        self.assertEqual(target["session_id"], "launch_id:" + launch)
        self.env.update(HGS_SESSION="codex/checker", HGS_RUN_ID=self.agent("codex/checker"))
        renamed.unlink()
        missing = self.hgs(*args)["tasks"][0]["target"]
        self.assertFalse(missing["resolved"])
        self.assertEqual(missing["session_id"], "launch_id:" + launch)

    def test_new_task_and_binding_arrivals_preserve_the_original_page_cursor(self):
        for n in range(5):
            self.add(f"Task {n}")
        args = ("list", "--project", "orchard", "--stage", "todo", "--limit", "2")
        first = self.hgs(*args)
        cursor = first["page"]["next"]
        second = self.hgs(*args, "--after", cursor)
        self.add("New at the beginning", "--position", "1")
        agent_record(self.state, self.env, "codex/new-binding", "new-run")
        again = self.hgs(*args, "--after", cursor)
        self.assertEqual(again["tasks"], second["tasks"])
        self.assertEqual(again["page"]["position"], second["page"]["position"] + 1)

    def test_the_watch_window_uses_the_same_bounded_page_routing(self):
        self.add("One window row")
        self.unrelated_records()
        # Warm the disposable routing cache through the real CLI first.
        expected = self.hgs("list", "--project", "orchard")["tasks"]
        stream = Stream(self, executable=HGS)
        stream.next(lambda frame: frame["t"] == "hello")
        # The stream's initial catalog discovery is independent of task
        # routing; complete it before accounting for the task subscription.
        stream.next(lambda f: f["t"] == "snapshot" and f.get("topic") == "catalog" and f["last"])
        io_path = Path(f"/proc/{stream.process.pid}/io")

        def read_count():
            return int(dict(line.split(":", 1) for line in io_path.read_text().splitlines())["rchar"])

        before = read_count()
        stream.send(op="subscribe", topic="tasks", project="project-1", window=2)
        rows = []
        while True:
            frame = stream.next(lambda f: f.get("topic") == "tasks")
            self.assertEqual(frame["t"], "snapshot", frame)
            rows += frame["items"]
            if frame["last"]:
                break
        self.assertEqual(rows, expected)
        read = read_count() - before
        self.assertLess(read, 2 * 1024 * 1024, f"a one-row task window read {read} unrelated bytes")

    def test_incomplete_discovery_never_reports_a_session_missing_and_background_refresh_finishes(self):
        task = self.add("Private author's reference")
        # More than one foreground refresh slice, without expensive job bodies.
        for n in range(300):
            name = f"codex/refresh/{n}"
            path = self.state / (hashlib.sha256(name.encode()).hexdigest() + ".json")
            path.write_text(json.dumps(dict(name=name, run_id=f"refresh-{n}")))
        args = ("list", "--project", "orchard", "--stage", "todo", "--limit", "1")
        code, stdout, stderr, _ = self.measured(*args)
        self.assertNotEqual(code, 0)
        self.assertIn("Task session routing is incomplete", stderr)
        self.assertNotIn('"resolved":false', stdout.replace(" ", ""))
        until = time.monotonic() + 15
        while True:
            code, stdout, stderr, _ = self.measured(*args)
            if code == 0:
                break
            self.assertIn("Task session routing is incomplete", stderr)
            self.assertLess(time.monotonic(), until, stderr)
            time.sleep(.05)
        row = json.loads(stdout)["tasks"][0]
        self.assertEqual(row["uuid"], task["uuid"])
        self.assertNotIn("resolved", row["author"])
        self.assertEqual(row["author"]["session"], "codex/private-page-reader")
