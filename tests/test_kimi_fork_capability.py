"""Kimi's external Fork needs the source instance closed, even at Ready."""
import json
import os
from pathlib import Path
import subprocess
import unittest
import uuid

import test_fork as fork
import test_pause as pause


REASON = "Pause or close this Kimi session before forking its saved context"
AGENT = fork.AGENT.replace('    parent = sys.argv[2]', '''    with (home / "native-forks.jsonl").open("a") as log:
        log.write(json.dumps(sys.argv[1:]) + "\\n")
    parent = sys.argv[2]''')


@unittest.skipUnless(os.name == "posix" and pause.TMUX, "requires private Unix tmux")
class KimiForkCapability(fork.Base):
    fork = fork.Forks.fork

    def setUp(self):
        original = pause.AGENT
        pause.AGENT = AGENT
        try:
            super().setUp()
        finally:
            pause.AGENT = original

    def test_live_ready_offer_explains_that_the_native_instance_must_close(self):
        name = self.start("kimi")
        state = json.loads(self.hgs("inspect", name))
        self.assertEqual(state["activity"], "idle")
        self.assertFalse(state["fork_supported"], state)
        self.assertEqual(state["fork_reason"], REASON)
        self.assertNotIn("fork", state["allowed_actions"])

    def test_live_direct_and_scoped_refusals_never_invoke_native_fork_or_change_source(self):
        name = self.start("kimi")
        source = self.binding(name)
        history = Path(source["transcript"]).read_bytes()
        ids = set((self.root / "history").glob("*.jsonl"))
        for dry in (False, True):
            result = subprocess.run([str(pause.HGS), "fork", name, "-n", "branch", "-d",
                                     *(["--dry-run"] if dry else [])],
                                    env=self.env, capture_output=True, text=True, timeout=30)
            self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
            self.assertIn(REASON, result.stderr)
        request = dict(request_id=str(uuid.uuid4()), action="fork", tag="branch",
                       expected_run_id=source["run_id"], expected_conversation_id=source["conversation_id"])
        replies = []
        for _ in range(2):
            result = subprocess.run([str(pause.HGS), "session-action", name, "--json"], env=self.env,
                                    input=json.dumps(request), capture_output=True, text=True, timeout=30)
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
            replies.append(json.loads(result.stdout))
        self.assertEqual(replies[0]["status"], "failed", replies[0])
        self.assertIn(REASON, replies[0]["error"])
        self.assertEqual(replies[0], replies[1])
        self.assertFalse((self.root / "native-forks.jsonl").exists())
        self.assertEqual(set((self.root / "history").glob("*.jsonl")), ids)
        self.assertEqual(Path(source["transcript"]).read_bytes(), history)
        for key in ("run_id", "conversation_id", "pid", "process_start"):
            self.assertEqual(self.binding(name)[key], source[key])
        self.assertNotEqual(subprocess.run([str(self.bin / "tmux"), "has-session", "-t", "=kimi/p/branch"],
                                          env=self.env, capture_output=True).returncode, 0)

    def test_explicit_pause_and_archive_keep_native_fork_available_without_resuming_source(self):
        name = self.start("kimi")
        self.hgs("pause", name)
        source = self.binding(name)
        self.assertTrue(json.loads(self.hgs("inspect", name))["fork_supported"])
        child = self.fork(name, "paused-branch")
        self.assertEqual(self.binding(child)["fork_parent_id"], source["conversation_id"])
        self.assertNotEqual(self.binding(child)["conversation_id"], source["conversation_id"])
        self.hgs("pause", child)
        self.hgs("archive", name)
        archived = next(json.loads(path.read_text()) for path in (self.root / "state/archives").glob("*.json"))
        self.assertTrue(json.loads(self.hgs("inspect", name, "--archive", archived["archive_id"]))["fork_supported"])
        self.hgs("fork", name, "-n", "archive-branch", "-d", "--archive", archived["archive_id"])
        self.wait(lambda: self.binding("kimi/p/archive-branch").get("conversation_id"))
        self.assertNotEqual(self.binding("kimi/p/archive-branch")["conversation_id"], source["conversation_id"])
        self.assertNotEqual(subprocess.run([str(self.bin / "tmux"), "has-session", "-t", "=" + name],
                                          env=self.env, capture_output=True).returncode, 0)


if __name__ == "__main__":
    unittest.main()
