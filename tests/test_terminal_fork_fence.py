"""Original-source Fork controls in private state, native stand-ins and tmux."""
import json
import os
from pathlib import Path
import subprocess
import unittest
import uuid

import test_fork as fork
import test_pause as pause
import test_saved_fresh_fence as fresh


AGENT = fork.AGENT.replace('    print("Forked to " + sid + " in 2ms")', '''    with (home / "native-forks.jsonl").open("a") as log:
        log.write(json.dumps({"cid": sid, "parent": parent}) + "\\n")
    plan = home / "fork-after-native.json"
    if plan.exists():
        change = json.loads(plan.read_text())
        slot = pathlib.Path(change["slot"])
        if change.get("fifo"):
            slot.unlink()
            os.mkfifo(slot, 0o600)
        else:
            slot.write_text(change["replacement"])
        if change.get("unknown"):
            print("Unconfirmed native fork outcome")
            sys.exit(1)
    print("Forked to " + sid + " in 2ms")''')


@unittest.skipUnless(os.name == "posix" and pause.TMUX, "requires private Unix tmux")
class TerminalForkSource(fork.Base):
    saved = fresh.SavedFreshSource.saved
    replace_on_final_writer = fresh.SavedFreshSource.replace_on_final_writer

    def setUp(self):
        original = pause.AGENT
        pause.AGENT = AGENT
        try:
            super().setUp()
        finally:
            pause.AGENT = original

    def forks(self):
        path = self.root / "native-forks.jsonl"
        return [json.loads(line) for line in path.read_text().splitlines()] if path.exists() else []

    def call(self, name, *args):
        return subprocess.run([str(pause.HGS), "fork", name, "-n", "branch", "-d", *args],
                              env=self.env, capture_output=True, text=True, timeout=40)

    def no_terminal(self, agent="kimi"):
        p = subprocess.run([str(self.bin / "tmux"), "has-session", "-t", "=" + agent + "/p/branch"],
                           env=self.env, capture_output=True)
        self.assertNotEqual(p.returncode, 0)

    def after_native(self, slot, replacement, unknown=False):
        (self.root / "fork-after-native.json").write_text(json.dumps(dict(
            slot=str(slot), replacement=replacement, unknown=unknown)))

    def scoped(self, name, original):
        request = dict(request_id=str(uuid.uuid4()), action="fork", tag="branch",
                       expected_run_id=original["run_id"], expected_conversation_id=original["conversation_id"])
        def call():
            result = subprocess.run([str(pause.HGS), "session-action", name, "--json"], env=self.env,
                                    input=json.dumps(request), capture_output=True, text=True, timeout=40)
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
            return json.loads(result.stdout)
        return request, call

    def test_changed_source_before_kimi_fork_creates_no_native_history(self):
        name, slot, original = self.saved("kimi")
        replacement = json.dumps(dict(original, future_field={"opaque": "changed"}))
        self.replace_on_final_writer(slot, replacement, at=1)
        result = self.call(name)
        self.assertTrue((self.root / "source-replaced.json").exists(), result.stdout + result.stderr)
        self.assertEqual(self.forks(), [], "the first admission must precede native history creation")
        self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
        self.assertIn("select the session again", result.stderr)
        self.assertEqual(slot.read_text(), replacement)
        self.no_terminal()

    def test_changed_source_after_confirmed_kimi_fork_keeps_exact_history_without_start(self):
        name, slot, original = self.saved("kimi")
        replacement = json.dumps(dict(original, future_field={"opaque": "after native fork"}))
        self.after_native(slot, replacement)
        result = self.call(name)
        self.assertEqual(len(self.forks()), 1, result.stdout + result.stderr)
        child = self.forks()[0]["cid"]
        self.assertNotEqual(child, original["conversation_id"])
        self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
        self.assertIn("select the session again", result.stderr)
        self.assertIn(child, result.stderr)
        self.assertTrue((self.root / "history" / (child + ".jsonl")).exists())
        self.assertEqual(slot.read_text(), replacement)
        self.no_terminal()

    def test_final_fifo_source_refuses_without_blocking_or_losing_native_cid(self):
        name, slot, original = self.saved("kimi")
        stored = slot.read_bytes()
        (self.root / "fork-after-native.json").write_text(json.dumps(dict(slot=str(slot), fifo=True)))
        try:
            try:
                result = subprocess.run([str(pause.HGS), "fork", name, "-n", "branch", "-d"],
                                        env=self.env, capture_output=True, text=True, timeout=5)
            except subprocess.TimeoutExpired:
                self.assertEqual(len(self.forks()), 1)
                self.no_terminal()
                print("FIFO_CONTROL_CONFIRMED_NATIVE_CID=" + self.forks()[0]["cid"], flush=True)
                raise
            self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
            self.assertEqual(len(self.forks()), 1)
            child = self.forks()[0]["cid"]
            self.assertIn(child, result.stderr)
            self.assertIn("select the session again", result.stderr)
            self.assertTrue((self.root / "history" / (child + ".jsonl")).exists())
            self.no_terminal()
        finally:
            if slot.exists():
                slot.unlink()
            slot.write_bytes(stored)

    def test_scoped_post_effect_uncertainty_and_exact_replay_never_fork_twice(self):
        name, slot, original = self.saved("kimi")
        self.after_native(slot, json.dumps(dict(original, future_field="changed")))
        request, call = self.scoped(name, original)
        receipt = call()
        self.assertEqual(receipt["request_id"], request["request_id"])
        self.assertEqual(receipt["status"], "uncertain", receipt)
        self.assertEqual(len(self.forks()), 1)
        child = self.forks()[0]["cid"]
        self.assertIn(child, receipt["error"])
        self.assertEqual(call(), receipt)
        self.assertEqual(len(self.forks()), 1)
        self.assertTrue((self.root / "history" / (child + ".jsonl")).exists())
        self.no_terminal()

    def test_scoped_pre_effect_failure_is_cached_without_native_fork(self):
        name, slot, original = self.saved("kimi")
        self.replace_on_final_writer(slot, json.dumps(dict(original, future_field="changed")), at=1)
        request, call = self.scoped(name, original)
        receipt = call()
        self.assertEqual(self.forks(), [])
        self.assertEqual(receipt["status"], "failed", receipt)
        self.assertEqual(receipt["request_id"], request["request_id"])
        self.assertEqual(call(), receipt)
        self.assertEqual(self.forks(), [])
        self.no_terminal()

    def test_unknown_native_fork_stays_unknown_without_replay_or_start(self):
        name, slot, original = self.saved("kimi")
        self.after_native(slot, slot.read_text(), unknown=True)
        _, call = self.scoped(name, original)
        receipt = call()
        self.assertEqual(receipt["status"], "uncertain", receipt)
        self.assertEqual(len(self.forks()), 1)
        child = self.forks()[0]["cid"]
        self.assertNotIn(child, receipt["error"], "the malformed response proves no confirmed native CID")
        self.assertEqual(call(), receipt)
        self.assertEqual(len(self.forks()), 1)
        self.assertTrue((self.root / "history" / (child + ".jsonl")).exists())
        self.no_terminal()

    def test_current_namespace_must_match_even_when_old_slot_is_unchanged(self):
        name, slot, original = self.saved()
        before = slot.read_bytes()
        self.replace_on_final_writer(slot, json.dumps(dict(original, base=["claude", "--effort", "low"])), at=1)
        path = self.root / "source-replace.json"
        plan = json.loads(path.read_text()); plan["route_change"] = True; path.write_text(json.dumps(plan))
        result = self.call(name, "--dry-run")
        self.assertTrue((self.root / "source-replaced.json").exists(), result.stdout + result.stderr)
        self.assertEqual(slot.read_bytes(), before)
        self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
        self.no_terminal("claude")


    def test_whole_source_missing_null_and_malformed_evidence_refuse(self):
        name, slot, original = self.saved("kimi")
        original["layout_transition"] = None
        absent = dict(original); del absent["layout_transition"]
        replacements = [None, "{malformed", json.dumps(absent)]
        replacements += [json.dumps(dict(original, **{key: value})) for key, value in [
            ("base", ["kimi", "--model", "changed"]), ("agent_home", str(self.root / "other-home")),
            ("account_id", "different"), ("launch_dir", str(self.root)),
            ("worktree_owner", {"opaque": "changed"}), ("worktree_protection", ["malformed"]),
            ("layout_transition", {"opaque": "negative evidence"}), ("future_field", [None, False])]]
        replacements.append(json.dumps(dict(original, padding="x" * (16 * 1024 * 1024))))
        for replacement in replacements:
            with self.subTest(replacement=None if replacement is None else replacement[:90]):
                slot.write_text(json.dumps(original))
                self.replace_on_final_writer(slot, replacement, at=1)
                result = self.call(name, "--dry-run")
                self.assertTrue((self.root / "source-replaced.json").exists())
                self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
                self.assertIn("select the session again", result.stderr)
                self.assertEqual(self.forks(), [])
                self.assertEqual(slot.read_text() if slot.exists() else None, replacement)
                self.no_terminal()

    def test_derived_subagent_projection_is_not_a_stored_source_change(self):
        name, slot, original = self.saved()
        original["subagents"] = {"historical-child": {"external": True, "state": "working"}}
        stored = json.dumps(original)
        slot.write_text(stored)
        observed = json.loads(self.hgs("inspect", name))
        self.assertNotEqual(observed.get("subagents"), original["subagents"],
                            "the control must actually produce a different derived view")
        result = self.call(name)
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        target = "claude/p/branch"
        self.wait(lambda: self.binding(target).get("conversation_id"))
        self.assertEqual(slot.read_text(), stored)
        self.assertEqual(self.binding(target)["fork_parent_id"], original["conversation_id"])

    def test_exact_id_missing_source_never_adopts_the_first_holder(self):
        name, label, original = self.saved()
        sid, _ = self.move_to_id_layout(name)
        slot = self.root / "state/sessions" / (sid + ".json")
        holder = json.dumps(dict(original, session_id=str(uuid.uuid4()), run_id=str(uuid.uuid4())))
        label.write_text(holder)
        self.replace_on_final_writer(slot, None, at=1)
        result = self.call(name, "--session-id", sid, "--dry-run")
        self.assertTrue((self.root / "source-replaced.json").exists())
        self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
        self.assertEqual(label.read_text(), holder)
        self.assertFalse(slot.exists())
        self.no_terminal("claude")

    def test_unchanged_canonical_sources_keep_parent_and_fork_context(self):
        for agent in ("claude", "codex", "kimi"):
            for live in (False, True):
                for layout in ("legacy", "id"):
                    with self.subTest(agent=agent, live=live, layout=layout):
                        name = self.start(agent, str(live).lower() + "-" + layout)
                        if not live:
                            self.hgs("pause", name)
                        if layout == "id":
                            sid, _ = self.move_to_id_layout(name)
                            slot = self.root / "state/sessions" / (sid + ".json")
                        else:
                            import hashlib
                            slot = self.root / "state" / (hashlib.sha256(name.encode()).hexdigest() + ".json")
                        original = json.loads(slot.read_text())
                        transcript = Path(original["transcript"])
                        body = json.loads(transcript.read_text()); body["context"] = ["original context"]
                        transcript.write_text(json.dumps(body) + "\n")
                        history = transcript.read_bytes()
                        canonical = json.dumps(dict(reversed(list(original.items()))), indent=2)
                        self.replace_on_final_writer(slot, canonical, at=1)
                        args = ("--session-id", original["session_id"]) if layout == "id" else ()
                        result = self.call(name, *args)
                        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
                        target = result.stdout.strip().split(" as ")[-1]
                        self.wait(lambda: self.binding(target).get("conversation_id") and
                                  self.binding(target).get("activity") == "idle")
                        child = self.binding(target)
                        for key in ("session_id", "run_id", "conversation_id", "pid", "process_start"):
                            self.assertEqual(self.binding(name).get(key), original.get(key), key)
                        self.assertNotEqual(child["session_id"], original["session_id"])
                        self.assertNotEqual(child["run_id"], original["run_id"])
                        self.assertNotEqual(child["conversation_id"], original["conversation_id"])
                        self.assertEqual(child["fork_parent_id"], original["conversation_id"])
                        self.assertEqual(json.loads(Path(child["transcript"]).read_text())["context"],
                                         ["original context"])
                        self.assertEqual(transcript.read_bytes(), history)
                        self.assertEqual(slot.read_text(), canonical)
                        (self.root / "source-replace.json").unlink()
                        self.hgs("pause", target)
                        self.hgs("archive", target)
                        if live:
                            self.hgs("pause", name)


if __name__ == "__main__":
    unittest.main()
