"""Legacy command migration through private CLI records; no agents or windows."""
import hashlib
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest
import uuid

HGS = Path(os.environ.get("HGS_TEST_BIN", Path(__file__).resolve().parents[1] / "target/debug/hgs"))


class ProcessCommands(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        root = Path(self.temp.name)
        self.state = root / "state"
        self.state.mkdir()
        (root / "tmux").mkdir()
        self.env = {key: value for key, value in os.environ.items() if not key.startswith("HGS_") and key not in ("TMUX", "TMUX_PANE")}
        self.env.update(HOME=str(root), HGS_STATE_DIR=str(self.state), HGS_CONFIG_DIR=str(root / "config"),
                        HGS_SELF="example", HGS_PEERS="", TMUX_TMPDIR=str(root / "tmux"))
        self.name = "codex/example/commands"
        self.command = "cat <<'EOF'\n" + "Ж heredoc body\n" * 1000 + "EOF"
        self.record = dict(version=1, name=self.name, agent="codex", run_id=str(uuid.uuid4()),
                           conversation_id=str(uuid.uuid4()), state="paused", cwd=str(root))
        job = dict(id="job-a", call_id="call-a", owner="main", provider="codex", source="hooks", command=self.command,
                   run_id=self.record["run_id"], conversation_id=self.record["conversation_id"], status="completed",
                   background=True, observed=True, output="retained native output", native_id="native-a",
                   capabilities=dict(output=True, stop=False, stdin=False))
        self.record["shell_jobs"] = {"job-a": job}

    def cli(self, *args):
        result = subprocess.run([str(HGS), *args], env=self.env, text=True, capture_output=True, timeout=15)
        self.assertEqual(result.returncode, 0, result.stderr)
        return result.stdout

    def check_record(self, path):
        saved = json.loads(path.read_text())
        job = saved["shell_jobs"]["job-a"]
        self.assertLessEqual(len(job["command"].encode()), 512)
        self.assertTrue(job["command_truncated"])
        self.assertEqual(job["command_digest"], hashlib.sha256(self.command.encode()).hexdigest())
        for key in ("id", "call_id", "run_id", "conversation_id", "status", "background", "observed", "output", "native_id", "capabilities"):
            self.assertEqual(job[key], self.record["shell_jobs"]["job-a"][key], key)
        return job

    def test_saved_rename_compacts_legacy_text_and_keeps_process_output(self):
        path = self.state / (hashlib.sha256(self.name.encode()).hexdigest() + ".json")
        original = json.dumps(self.record)
        path.write_text(original)
        reply = json.loads(self.cli("processes", self.name, "--json"))
        self.assertEqual(path.read_text(), original)
        self.assertTrue(reply["items"][0]["command_truncated"])
        self.assertIn("displayed preview", reply["notes"][0])
        renamed = "codex/example/renamed"
        self.cli("rename", self.name, renamed)
        new_path = self.state / (hashlib.sha256(renamed.encode()).hexdigest() + ".json")
        saved = self.check_record(new_path)
        output = json.loads(self.cli("processes", renamed, "--output", "job-a", "--run", self.record["run_id"],
                                     "--conversation", self.record["conversation_id"]))
        self.assertEqual(output["output"], saved["output"])
        self.assertFalse(output["truncated"])

    def test_archive_rename_compacts_legacy_text(self):
        archive_id = str(uuid.uuid4())
        self.record.update(archive_id=archive_id, archived_at=1, state="archived")
        (self.state / "archives").mkdir()
        path = self.state / "archives" / (archive_id + ".json")
        path.write_text(json.dumps(self.record))
        self.cli("rename", self.name, "codex/example/renamed", "--archive", archive_id)
        self.check_record(path)

    def test_settled_history_keeps_native_output_or_an_explicit_tail_and_refuses_pruned_jobs(self):
        root = Path(self.temp.name)
        self.name = "claude/example/commands"
        self.record["name"] = self.name
        self.record["agent"] = "claude"
        self.record["transcript"] = str(root / "native" / "transcript.jsonl")
        template = self.record["shell_jobs"]["job-a"]
        self.record["shell_jobs"] = {
            f"job-{i}": dict(template, id=f"job-{i}", call_id=f"call-{i}", updated_at=i,
                               ended_at=i, output="x" * 4096, output_truncated=False)
            for i in range(128)
        }
        native = root / "native" / self.record["conversation_id"] / "tool-results" / "result.txt"
        native.parent.mkdir(parents=True)
        native_text = "full native log text\n" * 300
        native.write_bytes(native_text.encode())
        self.record["shell_jobs"]["job-127"]["output_path"] = str(native)
        path = self.state / (hashlib.sha256(self.name.encode()).hexdigest() + ".json")
        path.write_text(json.dumps(self.record))
        renamed = "claude/example/retained"
        self.cli("rename", self.name, renamed)
        path = self.state / (hashlib.sha256(renamed.encode()).hexdigest() + ".json")
        original = path.read_text()
        saved = json.loads(original)
        self.assertLessEqual(len(saved["shell_jobs"]), 16)

        def output(job, ok=True):
            result = subprocess.run([str(HGS), "processes", renamed, "--output", job,
                                     "--run", self.record["run_id"], "--conversation", self.record["conversation_id"]],
                                    env=self.env, text=True, capture_output=True, timeout=15)
            if ok:
                self.assertEqual(result.returncode, 0, result.stderr)
                return json.loads(result.stdout)
            self.assertNotEqual(result.returncode, 0)
            return result.stderr

        self.assertEqual(output("job-127")["output"], native_text)
        tail = output("job-126")
        self.assertEqual(tail["output"], "x" * 1024)
        self.assertTrue(tail["truncated"])
        self.assertIn("no longer", output("job-0", ok=False))
        native.unlink()
        self.assertIn("log is no longer available", output("job-127", ok=False))
        self.assertEqual(path.read_text(), original, "output reads never rewrite the manifest")
