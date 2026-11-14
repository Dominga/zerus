"""Saved Start-fresh source controls; private state, agents and tmux only."""
import json
import os
import subprocess
import unittest
import uuid

import test_pause as pause


# A deterministic change from an existing terminal observation. The first
# writer read reconciles; the second is the final Start-fresh writer check,
# after the launch recipe and its existing current-record read. No test hook
# or timing sleep in production, and no other session's state is involved.
TERMINAL = r'''#!/usr/bin/env python3
import fcntl, json, os, pathlib, sys
root = pathlib.Path.home()
plan_path = root / "source-replace.json"
if sys.argv[1:2] == ["list-panes"] and plan_path.exists():
    with (root / "state/.lock").open("a") as lock:
        writer = False
        try:
            fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError:
            writer = True
        plan = json.loads(plan_path.read_text())
        if writer:
            plan["seen"] += 1
        if not plan.get("done") and ((writer and plan["seen"] == plan["at"])
                                     or (not writer and plan["at"] == 0)):
            slot = pathlib.Path(plan["slot"])
            before = slot.read_text()
            if plan.get("route_change"):
                (root / "state/bindings").mkdir(exist_ok=True)
                (root / "state/name_routes").mkdir(exist_ok=True)
                (root / "state/bindings" / slot.name).write_text(plan["replacement"])
                (root / "state/name_routes" / slot.name).write_text("{}")
            elif plan["replacement"] is None:
                slot.unlink()
            else:
                slot.write_text(plan["replacement"])
            plan["done"] = True
            (root / "source-replaced.json").write_text(json.dumps({"before": before}))
        plan_path.write_text(json.dumps(plan))
os.execv(__TMUX__, [__TMUX__, "-S", __SOCKET__, "-f", "/dev/null", *sys.argv[1:]])
'''


@unittest.skipUnless(os.name == "posix" and pause.TMUX, "requires private Unix tmux")
class SavedFreshSource(pause.Harness):
    def saved(self, agent="claude"):
        name = self.start(agent)
        self.hgs("pause", name)
        paths = [*(self.root / "state").glob("*.json"),
                 *(self.root / "state/bindings").glob("*.json")]
        slot = next(path for path in paths if json.loads(path.read_text()).get("name") == name)
        return name, slot, json.loads(slot.read_text())

    def replace_on_final_writer(self, slot, replacement, at=2):
        (self.root / "source-replaced.json").unlink(missing_ok=True)
        (self.root / "source-replace.json").write_text(json.dumps({
            "slot": str(slot), "replacement": replacement, "at": at, "seen": 0,
        }))
        self.script("tmux", TERMINAL.replace("__TMUX__", repr(pause.TMUX))
                    .replace("__SOCKET__", repr(self.socket)))

    def fresh(self, name, *args):
        return subprocess.run([str(pause.HGS), "resume", name, "--fresh", "-d", *args],
                              env=self.env, capture_output=True, text=True, timeout=20)

    def assert_refused(self, name, slot, replacement, *args, at=2):
        self.replace_on_final_writer(slot, replacement, at)
        launches = (self.root / "argv.jsonl").read_bytes()
        result = self.fresh(name, *args)
        self.assertTrue((self.root / "source-replaced.json").exists(), result.stdout + result.stderr)
        self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
        self.assertIn("select the session again", result.stderr)
        self.assertEqual(slot.read_text() if slot.exists() else None, replacement)
        self.assertEqual((self.root / "argv.jsonl").read_bytes(), launches)
        terminal = subprocess.run([str(self.bin / "tmux"), "has-session", "-t", "=" + name],
                                  env=self.env, capture_output=True)
        self.assertNotEqual(terminal.returncode, 0)

    def test_same_identity_recipe_change_refuses_before_native_start(self):
        name, slot, original = self.saved()
        changed = dict(original, base=["claude", "--effort", "low"])
        replacement = json.dumps(changed)
        self.assert_refused(name, slot, replacement)

    def test_recipe_preparation_uses_the_original_captured_frame(self):
        name, slot, original = self.saved()
        changed = dict(original, base=["claude", "--effort", "low"])
        # The unlocked terminal observation inside session_spec happens after
        # capture, while preparing the recipe, before its final writer lock.
        self.assert_refused(name, slot, json.dumps(changed), "--dry-run", at=0)

    def test_complete_source_changes_include_unknown_and_negative_evidence(self):
        name, slot, original = self.saved()
        original.update(layout_transition={"opaque": "original"}, future_field={"bind": [1, 2]})
        changes = [("agent_home", str(self.root / "another-home")),
                   ("launch_dir", str(self.root)), ("account", {"id": "different"}),
                   ("future_field", {"bind": [2, 1]}), ("worktree_owner", None),
                   ("worktree_protection", ["unknown"]), ("layout_transition", None),
                   ("layout_transition", ["malformed"])]
        for key, value in changes:
            with self.subTest(key=key, value=value):
                slot.write_text(json.dumps(original))
                self.assert_refused(name, slot, json.dumps(dict(original, **{key: value})), "--dry-run")

    def test_missing_malformed_and_null_to_absent_sources_refuse(self):
        name, slot, original = self.saved()
        original["layout_transition"] = None
        removed = dict(original)
        del removed["layout_transition"]
        for replacement in (None, "{malformed", json.dumps(removed)):
            with self.subTest(replacement=replacement):
                slot.write_text(json.dumps(original))
                self.assert_refused(name, slot, replacement, "--dry-run")

    def test_unchanged_canonical_source_starts_fresh_on_every_provider(self):
        for agent in ("claude", "codex", "kimi"):
            with self.subTest(agent=agent):
                name, slot, original = self.saved(agent)
                original["layout_transition"] = {"opaque": "retained"}
                slot.write_text(json.dumps(original))
                replacement = json.dumps(dict(reversed(list(original.items()))), indent=3)
                self.replace_on_final_writer(slot, replacement)
                result = self.fresh(name, "--expected-run-id", original["run_id"])
                self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
                self.wait(lambda: self.binding(name).get("conversation_id") not in
                          (None, original["conversation_id"]) and self.binding(name).get("activity") == "idle")
                current = self.binding(name)
                self.assertNotEqual(current["run_id"], original["run_id"])
                for key in ("session_id", "journal_name", "launch_label", "layout_transition"):
                    self.assertEqual(current[key], original[key], key)
                self.assertEqual(current["session_clear"]["previous_conversation_id"], original["conversation_id"])
                self.assertEqual(len((self.root / "argv.jsonl").read_text().splitlines()),
                                 2 * (1 + ("claude", "codex", "kimi").index(agent)))
                (self.root / "source-replace.json").unlink()
                self.hgs("pause", name)

    def test_exact_id_source_never_falls_back_to_the_first_label_holder(self):
        name, label, original = self.saved()
        sid, _ = self.move_to_id_layout(name)
        slot = self.root / "state/sessions" / (sid + ".json")
        holder = dict(original, session_id=str(uuid.uuid4()), run_id=str(uuid.uuid4()),
                      conversation_id=str(uuid.uuid4()))
        first = json.dumps(holder)
        label.write_text(first)
        result = self.fresh(name, "--session-id", sid, "--dry-run")
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertEqual(label.read_text(), first)
        self.assert_refused(name, slot, None, "--session-id", sid, "--dry-run")
        self.assertEqual(label.read_text(), first)

    def test_route_change_keeps_the_existing_final_namespace_guard(self):
        name, slot, original = self.saved()
        before = slot.read_bytes()
        replacement = dict(original, run_id=str(uuid.uuid4()), session_id=str(uuid.uuid4()))
        self.replace_on_final_writer(slot, json.dumps(replacement), at=0)
        plan_path = self.root / "source-replace.json"
        plan = json.loads(plan_path.read_text())
        plan["route_change"] = True
        plan_path.write_text(json.dumps(plan))
        launches = (self.root / "argv.jsonl").read_bytes()
        result = self.fresh(name, "--dry-run")
        self.assertTrue((self.root / "source-replaced.json").exists(), result.stdout + result.stderr)
        self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
        self.assertEqual(slot.read_bytes(), before, "the selected old source itself did not change")
        self.assertEqual((self.root / "argv.jsonl").read_bytes(), launches)

    def test_relative_state_root_keeps_an_unchanged_selected_id_recipe(self):
        name, _, original = self.saved()
        sid, _ = self.move_to_id_layout(name)
        env = dict(self.env, HGS_STATE_DIR="state")
        result = subprocess.run([str(pause.HGS), "resume", "--session-id", sid, "--fresh",
                                 "--dry-run"], cwd=self.root, env=env,
                                capture_output=True, text=True, timeout=20)
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertIn("HGS_FRESH_REPLACES=" + original["run_id"], result.stdout)
        self.assertEqual(self.binding(name)["run_id"], original["run_id"])


if __name__ == "__main__":
    unittest.main()
