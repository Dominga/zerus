#!/usr/bin/env python3
"""Zerus's SessionStart hook gives Claude Code and Codex sessions the project
task instructions: fake agents through the real lifecycle."""
import json
from pathlib import Path
import subprocess
import sys
import unittest

import test_fork


class TaskContext(test_fork.Base):
    def hook_output(self, name, agent, kind="SessionStart", raw=False, **fields):
        record = self.binding(name)
        event = dict(session_id=record["conversation_id"], hook_event_name=kind, **fields)
        env = dict(self.env, HGS_SESSION=name, HGS_RUN_ID=record["run_id"], HGS_AGENT=agent)
        if not raw:
            return self.hook(event, env).stdout
        legacy = test_fork.test_pause.LEGACY_STATE
        command = ([sys.executable, legacy, "hook"] if legacy else
                   [str(test_fork.test_pause.HGS), "__state", "hook"])
        return subprocess.run(command, env=env, input=json.dumps(event).encode("utf-8"),
                              capture_output=True, check=True, timeout=15).stdout

    def test_claude_and_codex_sessions_read_the_task_commands_at_start(self):
        for agent in ("claude", "codex"):
            with self.subTest(agent=agent):
                name = self.start(agent, "context")
                output = json.loads(self.hook_output(name, agent, source="startup"))["hookSpecificOutput"]
                self.assertEqual(output["hookEventName"], "SessionStart")
                self.assertIn("hgs task add", output["additionalContext"])
                self.assertIn("поставь таск", output["additionalContext"])
                self.assertIn("hgs guide tasks", output["additionalContext"])
                self.assertIn("hgs task report-attempt <ref> --attempt UUID", output["additionalContext"])
                self.assertIn("never substitute the latest attempt ID", output["additionalContext"])
                # Tracking goes on: the session is still idle in its conversation.
                self.assertEqual(self.binding(name)["activity"], "idle")

    def test_enabled_hint_and_tasks_have_one_exact_complete_stdout_document(self):
        expected = Path(__file__).with_name("fixtures").joinpath("session-start-context.json").read_bytes()
        for agent in ("claude", "codex"):
            with self.subTest(agent=agent):
                # Start without a hint mark, then exercise the actual complete
                # hook entry with both admitted contexts enabled.
                self.env["HGS_AGENT_HINT"] = "0"
                name = self.start(agent, "combined-context")
                self.env.update(HGS_AGENT_HINT="1", HGS_AGENT_HINT_OFF="")
                raw = self.hook_output(name, agent, source="startup", raw=True)
                value = json.loads(raw)  # Reject trailing/second JSON documents.
                text = value["hookSpecificOutput"]["additionalContext"]
                self.assertIn("Run hgs guide before using it.", text)
                self.assertIn("Run `hgs task` in your shell", text)
                self.assertEqual(raw, expected)
                self.assertEqual(raw.count(b"\n"), 1)
                self.assertFalse(raw.startswith(b"\xef\xbb\xbf"))
                # The durable once-only hint mark survives; tasks still arrive.
                again = json.loads(self.hook_output(name, agent, source="resume"))
                self.assertNotIn("Run hgs guide before using it.", again["hookSpecificOutput"]["additionalContext"])
                self.assertIn("Run `hgs task` in your shell", again["hookSpecificOutput"]["additionalContext"])

    def test_a_start_hgs_did_not_record_reads_nothing(self):
        name = self.start("claude", "unknown")
        record = self.binding(name)
        event = dict(session_id=record["conversation_id"], hook_event_name="SessionStart", source="startup")
        env = dict(self.env, HGS_SESSION=name, HGS_RUN_ID="run-that-is-not-live", HGS_AGENT="claude")
        self.assertEqual(self.hook(event, env).stdout, "")

    def test_subagents_other_agents_and_other_events_read_nothing(self):
        name = self.start("claude", "quiet")
        self.assertEqual(self.hook_output(name, "claude", agent_id="agent-1"), "")
        self.assertEqual(self.hook_output(name, "claude", kind="UserPromptSubmit", prompt="hello"), "")
        kimi = self.start("kimi", "plain")
        self.assertEqual(self.hook_output(kimi, "kimi"), "")


if __name__ == "__main__":
    unittest.main()
