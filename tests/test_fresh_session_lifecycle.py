"""First-turn lifecycle with real hgs/tmux and private supervised fake agents."""
import json
import subprocess
import unittest
import uuid

import test_pause


@unittest.skipUnless(test_pause.TMUX, "tmux is required for isolated lifecycle tests")
class FreshLifecycle(test_pause.Harness):
    def setUp(self):
        super().setUp()
        # Use ordinary supported launch arguments. Only this fixture's hooks
        # are withheld, as in a TUI which has not emitted its first event yet.
        before = 'if "--no-hook" in args:'
        self.assertEqual(test_pause.AGENT.count(before), 1)
        source = test_pause.AGENT.replace(before, 'if (home / "hold-hooks").exists():')
        for agent in ("codex", "claude", "kimi"):
            self.script(agent, source)
        (self.root / "hold-hooks").touch()

    def cold(self, agent, tag):
        name = f"{agent}/p/{tag}"
        self.hgs(agent, "p", "-n", tag, "-d")
        self.wait(lambda: self.binding(name).get("pid") not in
                  (None, self.binding(name).get("supervisor", {}).get("pid")))
        record = self.binding(name)
        self.assertIsNone(record["conversation_id"])
        self.assertFalse(record.get("last_event_at"))
        self.assertEqual(record["activity"], "unknown")
        return name, record

    def request(self, record, action):
        return dict(request_id=str(uuid.uuid4()), action=action,
                    expected_run_id=record["run_id"], expected_conversation_id="")

    def scoped(self, name, request):
        result = subprocess.run([str(test_pause.HGS), "session-action", name, "--json"],
                                input=json.dumps(request), env=self.env,
                                text=True, capture_output=True, timeout=20)
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        return json.loads(result.stdout)

    def test_pause_before_any_native_event_keeps_the_original_binding(self):
        for agent in ("codex", "claude", "kimi"):
            with self.subTest(agent=agent):
                name, original = self.cold(agent, "fresh-pause")
                request = self.request(original, "pause")
                result = self.scoped(name, request)
                self.assertEqual(result["status"], "completed", result)
                paused = self.binding(name)
                self.assertEqual(paused["run_id"], original["run_id"])
                self.assertIsNone(paused["conversation_id"])
                self.assertTrue(paused["paused"])
                self.assertFalse(paused.get("last_event_at"))
                self.assertEqual(self.scoped(name, request), result)
                terminal = subprocess.run([str(self.bin / "tmux"), "has-session", "-t", "=" + name],
                                          env=self.env, capture_output=True)
                self.assertNotEqual(terminal.returncode, 0)
                self.assertTrue(self.project.is_dir())

    def test_terminate_without_a_first_event_archives_once_and_keeps_the_folder(self):
        name, original = self.cold("codex", "fresh-terminate")
        request = self.request(original, "terminate")
        result = self.scoped(name, request)
        self.assertEqual(result["status"], "completed", result)
        self.assertEqual(result["run_id"], original["run_id"])
        self.assertEqual(result["conversation_id"], "")
        archive = self.root / "state/archives" / (result["result_target"]["archive_id"] + ".json")
        before = archive.read_bytes()
        archived = json.loads(before)
        self.assertEqual(archived["run_id"], original["run_id"])
        self.assertIsNone(archived["conversation_id"])
        self.assertFalse(self.binding(name))
        self.assertEqual(self.scoped(name, request), result)
        self.assertEqual(archive.read_bytes(), before)
        self.assertEqual(len(list((self.root / "state/archives").glob("*.json"))), 1)
        self.assertTrue(self.project.is_dir())
        self.assertFalse((self.project / ".git").exists())

    def test_a_first_native_event_refuses_the_old_empty_binding_before_any_stop(self):
        name, original = self.cold("codex", "first-event-race")
        conversation = str(uuid.uuid4())
        history = self.root / "history" / (conversation + ".jsonl")
        history.parent.mkdir(exist_ok=True)
        history.write_text(json.dumps({"session_id": conversation}) + "\n")
        self.send_event(name, "SessionStart", session_id=conversation, source="startup",
                        cwd=str(self.project), transcript_path=str(history))
        self.assertEqual(self.binding(name)["conversation_id"], conversation)
        for action in ("pause", "terminate"):
            result = self.scoped(name, self.request(original, action))
            self.assertEqual(result["status"], "failed", result)
            after = self.binding(name)
            self.assertEqual((after["pid"], after["process_start"]),
                             (original["pid"], original["process_start"]))
            self.t("has-session", "-t", "=" + name)
        self.assertTrue(self.project.is_dir())

    def test_plain_pause_accepts_the_original_unconfirmed_fresh_run(self):
        name, original = self.cold("codex", "plain-pause")
        self.hgs("pause", name)
        paused = self.binding(name)
        self.assertTrue(paused["paused"])
        self.assertEqual(paused["run_id"], original["run_id"])
        self.assertIsNone(paused["conversation_id"])

    def test_a_launch_prompt_is_not_treated_as_a_pre_input_idle_session(self):
        name = "codex/p/launch-prompt"
        self.hgs("codex", "p", "-n", "launch-prompt", "-d", "--", "Original launch prompt")
        self.wait(lambda: self.binding(name).get("pid") not in
                  (None, self.binding(name).get("supervisor", {}).get("pid")))
        original = self.binding(name)
        self.assertIsNone(original["conversation_id"])
        result = self.scoped(name, self.request(original, "pause"))
        self.assertEqual(result["status"], "failed", result)
        self.t("has-session", "-t", "=" + name)
        self.assertEqual(self.binding(name)["pid"], original["pid"])


if __name__ == "__main__":
    unittest.main()
