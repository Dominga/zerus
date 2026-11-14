"""Ordinary saved Resume source controls; private state/agents/tmux only."""
import json
import os
import subprocess
import unittest
import uuid

import test_pause as pause
import test_saved_fresh_fence as fresh


@unittest.skipUnless(os.name == "posix" and pause.TMUX, "requires private Unix tmux")
class SavedResumeSource(pause.Harness):
    def replace_on_final_writer(self, slot, replacement, at=2):
        fresh.SavedFreshSource.replace_on_final_writer(self, slot, replacement, at)
        # Ordinary Resume also samples the terminal before capturing its raw
        # frame. A negative step selects its Nth unlocked observation.
        script = self.bin / "tmux"
        text = script.read_text().replace('plan = json.loads(plan_path.read_text())',
            'plan = json.loads(plan_path.read_text())\n        if not writer: plan["unlocked"] = plan.get("unlocked", 0) + 1')
        text = text.replace('(not writer and plan["at"] == 0)',
            '(not writer and (plan["at"] == 0 or plan.get("unlocked") == -plan["at"]))')
        script.write_text(text)

    def saved(self, agent="claude", tag="one"):
        name = self.start(agent, tag)
        self.hgs("pause", name)
        paths = [*(self.root / "state").glob("*.json"),
                 *(self.root / "state/bindings").glob("*.json")]
        slot = next(p for p in paths if json.loads(p.read_text()).get("name") == name)
        return name, slot, json.loads(slot.read_text())

    def resume_call(self, *args):
        return subprocess.run([str(pause.HGS), "resume", *args, "-d"], env=self.env,
                              capture_output=True, text=True, timeout=25)

    def test_same_identity_recipe_change_refuses_before_resume_start(self):
        name, slot, original = self.saved()
        changed = dict(original, base=["claude", "--effort", "low"])
        replacement = json.dumps(changed)
        self.replace_on_final_writer(slot, replacement)
        launches = (self.root / "argv.jsonl").read_bytes()
        result = self.resume_call(name)
        self.assertTrue((self.root / "source-replaced.json").exists(), result.stdout + result.stderr)
        self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
        self.assertIn("select the session again", result.stderr)
        self.assertEqual(slot.read_text(), replacement)
        self.assertEqual((self.root / "argv.jsonl").read_bytes(), launches)
        terminal = subprocess.run([str(self.bin / "tmux"), "has-session", "-t", "=" + name],
                                  env=self.env, capture_output=True)
        self.assertNotEqual(terminal.returncode, 0)

    def test_later_all_refusal_never_rolls_back_restarts_or_replays_earlier_resume(self):
        first, _, before_first = self.saved(tag="alpha")
        later, slot, before_later = self.saved(tag="omega")
        replacement = json.dumps(dict(before_later, future_field={"opaque": "changed"}))
        # Reconcile is writer read 1; the first final check is 2, later is 3.
        self.replace_on_final_writer(slot, replacement, at=3)
        launches = (self.root / "argv.jsonl").read_text().splitlines()
        result = self.resume_call("--all")
        self.assertTrue((self.root / "source-replaced.json").exists(), result.stdout + result.stderr)
        self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
        self.assertIn("select the session again", result.stderr)
        self.wait(lambda: self.binding(first).get("run_id") not in (None, before_first["run_id"])
                  and self.binding(first).get("activity") == "idle")
        resumed = self.binding(first)
        self.assertEqual(resumed["session_id"], before_first["session_id"])
        self.assertEqual(resumed["conversation_id"], before_first["conversation_id"])
        self.t("has-session", "-t", "=" + first)
        self.assertEqual(slot.read_text(), replacement)
        self.assertEqual(self.binding(later)["run_id"], before_later["run_id"])
        after = (self.root / "argv.jsonl").read_text().splitlines()
        self.assertEqual(after[:len(launches)], launches)
        self.assertEqual(len(after), len(launches) + 1, "one earlier start, no replay or later start")
        self.assertIn(before_first["conversation_id"], json.loads(after[-1])["argv"])
        terminal = subprocess.run([str(self.bin / "tmux"), "has-session", "-t", "=" + later],
                                  env=self.env, capture_output=True)
        self.assertNotEqual(terminal.returncode, 0)

    def test_scoped_refusal_keeps_one_failed_receipt_without_replay(self):
        name, slot, original = self.saved()
        replacement = json.dumps(dict(original, future_field={"opaque": "changed"}))
        self.replace_on_final_writer(slot, replacement, at=1)
        launches = (self.root / "argv.jsonl").read_bytes()
        request = dict(request_id=str(uuid.uuid4()), action="resume",
                       expected_run_id=original["run_id"], expected_conversation_id=original["conversation_id"])
        def call():
            result = subprocess.run([str(pause.HGS), "session-action", name, "--json"], env=self.env,
                                    input=json.dumps(request), capture_output=True, text=True, timeout=25)
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
            return json.loads(result.stdout)
        receipt = call()
        self.assertEqual(receipt["request_id"], request["request_id"])
        self.assertEqual(receipt["status"], "failed", receipt)
        self.assertIn("select the session again", receipt["error"])
        self.assertEqual(call(), receipt)
        self.assertEqual(json.loads((self.root / "source-replace.json").read_text())["seen"], 1)
        self.assertEqual(slot.read_text(), replacement)
        self.assertEqual((self.root / "argv.jsonl").read_bytes(), launches)

    def test_preparation_change_cannot_refresh_the_retained_fence(self):
        name, slot, original = self.saved()
        replacement = json.dumps(dict(original, base=["claude", "--effort", "low"]))
        self.replace_on_final_writer(slot, replacement, at=-2)
        result = self.resume_call(name, "--dry-run")
        self.assertTrue((self.root / "source-replaced.json").exists(), result.stdout + result.stderr)
        self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
        self.assertIn("select the session again", result.stderr)
        self.assertEqual(slot.read_text(), replacement)

    def test_namespace_guard_stays_when_the_original_slot_is_unchanged(self):
        name, slot, original = self.saved()
        before = slot.read_bytes()
        self.replace_on_final_writer(slot, json.dumps(dict(original, run_id=str(uuid.uuid4()),
                                     session_id=str(uuid.uuid4()))), at=-2)
        plan_path = self.root / "source-replace.json"
        plan = json.loads(plan_path.read_text()); plan["route_change"] = True
        plan_path.write_text(json.dumps(plan))
        result = self.resume_call(name, "--dry-run")
        self.assertTrue((self.root / "source-replaced.json").exists(), result.stdout + result.stderr)
        self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
        self.assertEqual(slot.read_bytes(), before)

    def test_whole_source_and_missing_malformed_slots_require_reselection(self):
        name, slot, original = self.saved()
        original["layout_transition"] = None
        missing = dict(original); del missing["layout_transition"]
        values = [None, "{malformed", json.dumps(missing)]
        values += [json.dumps(dict(original, **{key: value})) for key, value in [
            ("agent_home", str(self.root / "other-home")), ("launch_dir", str(self.root)),
            ("worktree_owner", {"unknown": "changed"}), ("worktree_protection", ["malformed"]),
            ("future_field", {"opaque": [None, False]}), ("layout_transition", {"new": "evidence"})]]
        for replacement in values:
            with self.subTest(replacement=replacement):
                slot.write_text(json.dumps(original))
                self.replace_on_final_writer(slot, replacement)
                result = self.resume_call(name, "--dry-run")
                self.assertTrue((self.root / "source-replaced.json").exists(), result.stdout + result.stderr)
                self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
                self.assertIn("select the session again", result.stderr)
                self.assertEqual(slot.read_text() if slot.exists() else None, replacement)

    def test_exact_id_never_adopts_a_first_label_holder(self):
        name, label, original = self.saved()
        sid, _ = self.move_to_id_layout(name)
        slot = self.root / "state/sessions" / (sid + ".json")
        first = json.dumps(dict(original, session_id=str(uuid.uuid4()), run_id=str(uuid.uuid4())))
        label.write_text(first)
        self.replace_on_final_writer(slot, None)
        result = self.resume_call(name, "--session-id", sid, "--dry-run")
        self.assertTrue((self.root / "source-replaced.json").exists(), result.stdout + result.stderr)
        self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
        self.assertEqual(label.read_text(), first)
        self.assertFalse(slot.exists())

    def test_unchanged_canonical_source_resumes_each_provider_and_layout(self):
        for agent in ("claude", "codex", "kimi"):
            for layout in ("legacy", "id"):
                with self.subTest(agent=agent, layout=layout):
                    name, slot, original = self.saved(agent, layout)
                    if layout == "id":
                        sid, _ = self.move_to_id_layout(name)
                        slot = self.root / "state/sessions" / (sid + ".json")
                        original = json.loads(slot.read_text())
                    original["layout_transition"] = {"opaque": "retain"}
                    slot.write_text(json.dumps(original))
                    replacement = json.dumps(dict(reversed(list(original.items()))), indent=3)
                    self.replace_on_final_writer(slot, replacement)
                    result = self.resume_call(name)
                    self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
                    self.wait(lambda: self.binding(name).get("run_id") not in (None, original["run_id"])
                              and self.binding(name).get("activity") == "idle")
                    resumed = self.binding(name)
                    for key in ("session_id", "conversation_id", "journal_name", "launch_label", "layout_transition"):
                        self.assertEqual(resumed[key], original[key], key)
                    self.assertEqual(resumed.get("layout"), original.get("layout"))
                    (self.root / "source-replace.json").unlink()
                    self.hgs("pause", name)


if __name__ == "__main__":
    unittest.main()
