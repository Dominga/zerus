#!/usr/bin/env python3
"""Restore source controls with private state, fake agents and a private tmux."""
import json
import os
import subprocess
import unittest
import uuid

import test_pause as pause


# Replace the selected archive from the existing terminal read, while the CLI
# holds its state writer. Ordinary resume first reconciles (read 1), then
# checks pre-restore (2) and pre-start (3). Scoped restore skips reconciliation.
# No sleeps, production hooks or owner's state involved.
TERMINAL = r'''#!/usr/bin/env python3
import fcntl, json, os, pathlib, sys
root = pathlib.Path.home()
plan_path = root / "archive-replace.json"
if sys.argv[1:2] == ["list-panes"] and plan_path.exists():
    with (root / "state/.lock").open("a") as lock:
        try:
            fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError:
            plan = json.loads(plan_path.read_text())
            plan["seen"] += 1
            if plan["seen"] == plan["at"]:
                archive = pathlib.Path(plan["archive"])
                before = archive.read_text()
                if plan["replacement"] is None:
                    archive.unlink()
                else:
                    archive.write_text(plan["replacement"])
                (root / "archive-replaced.json").write_text(json.dumps({"before": before, "at": plan["at"]}))
            plan_path.write_text(json.dumps(plan))
os.execv(__TMUX__, [__TMUX__, "-S", __SOCKET__, "-f", "/dev/null", *sys.argv[1:]])
'''


@unittest.skipUnless(os.name == "posix" and pause.TMUX, "requires private Unix tmux")
class ArchiveRestoreSource(pause.Harness):
    rows = pause.Archive.rows
    archived = pause.Archive.archived
    save_archive = pause.Archive.save_archive

    def replace_on_writer_read(self, archive, replacement, at):
        (self.root / "archive-replace.json").write_text(json.dumps({
            "archive": str(archive), "replacement": replacement, "at": at, "seen": 0,
        }))
        self.script("tmux", TERMINAL.replace("__TMUX__", repr(pause.TMUX))
                    .replace("__SOCKET__", repr(self.socket)))

    def test_changed_archive_with_same_run_and_conversation_refuses_before_start(self):
        name, _, selected = self.save_archive()
        archive = self.root / "state/archives" / (selected["archive_id"] + ".json")
        original = json.loads(archive.read_text())
        changed = dict(original, layout_transition={"opaque": "new negative evidence"})
        replacement = json.dumps(changed)
        self.replace_on_writer_read(archive, replacement, at=3)
        launches = (self.root / "argv.jsonl").read_text()
        result = subprocess.run([str(pause.HGS), "resume", name, "--archive", selected["archive_id"], "-d"],
                                env=self.env, capture_output=True, text=True, timeout=20)
        self.assertTrue((self.root / "archive-replaced.json").is_file(), result.stdout + result.stderr)
        self.assertEqual(changed["run_id"], original["run_id"])
        self.assertEqual(changed["conversation_id"], original["conversation_id"])
        self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
        self.assertIn("select the archive again", result.stderr)
        self.assertEqual(archive.read_text(), replacement)
        self.assertFalse(self.binding(name))
        self.assertEqual((self.root / "argv.jsonl").read_text(), launches)

    def test_change_is_checked_before_attempting_worktree_restore(self):
        name, _, selected = self.save_archive()
        archive = self.root / "state/archives" / (selected["archive_id"] + ".json")
        original = json.loads(archive.read_text())
        # An unchanged selection with this missing folder must reach today's
        # worktree refusal. A changed one must refuse before entering Restore.
        original["launch_dir"] = str(self.root / "missing-checkout")
        archive.write_text(json.dumps(original))
        changed = dict(original, worktree_protection={"opaque": "changed"})
        replacement = json.dumps(changed)
        self.replace_on_writer_read(archive, replacement, at=2)
        result = subprocess.run([str(pause.HGS), "resume", name, "--archive", selected["archive_id"], "-d"],
                                env=self.env, capture_output=True, text=True, timeout=20)
        self.assertTrue((self.root / "archive-replaced.json").is_file(), result.stdout + result.stderr)
        self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
        self.assertIn("select the archive again", result.stderr)
        self.assertEqual(archive.read_text(), replacement)
        self.assertFalse(self.binding(name))

    def test_scoped_changes_keep_one_failed_receipt_and_never_start_or_replay(self):
        name, _, selected = self.save_archive()
        archive = self.root / "state/archives" / (selected["archive_id"] + ".json")
        original = json.loads(archive.read_text())
        original.update(layout="id", terminal_name="previous-terminal",
                        future_unknown_field={"must": ["bind", "original"]},
                        layout_transition={"opaque": {"must": "retain"}})
        cases = [
            ("session_id", str(uuid.uuid4())),
            ("layout", "changed"),
            ("terminal_name", "changed"),
            ("layout_transition", None),
            ("layout_transition", ["malformed"]),
            ("worktree_owner", {"nonce": "changed"}),
            ("worktree_protection", {"nonce": "changed"}),
            ("base", ["claude", "changed-recipe"]),
            ("agent_home", str(self.root / "different-home")),
            ("future_unknown_field", {"must": ["bind", "everything"]}),
        ]
        launches = (self.root / "argv.jsonl").read_text()
        for at in (1, 2):
            for key, value in cases:
                with self.subTest(at=at, key=key):
                    archive.write_text(json.dumps(original))
                    changed = dict(original, **{key: value})
                    replacement = json.dumps(changed)
                    self.replace_on_writer_read(archive, replacement, at)
                    payload = dict(request_id=str(uuid.uuid4()), action="restore", archive_id=selected["archive_id"],
                                   expected_run_id=original["run_id"], expected_conversation_id=original["conversation_id"])
                    command = [str(pause.HGS), "session-action", name, "--json"]
                    def call():
                        result = subprocess.run(command, input=json.dumps(payload), env=self.env,
                                                capture_output=True, text=True, timeout=20)
                        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
                        return json.loads(result.stdout)
                    receipt = call()
                    self.assertEqual(receipt["status"], "failed", receipt)
                    self.assertIn("select the archive again", receipt["error"])
                    self.assertEqual(receipt["request_id"], payload["request_id"])
                    self.assertEqual(call(), receipt, "the same request must never be replayed")
                    self.assertEqual(json.loads((self.root / "archive-replace.json").read_text())["seen"], at)
                    self.assertEqual(archive.read_text(), replacement)
                    self.assertFalse(self.binding(name))
                    self.assertEqual((self.root / "argv.jsonl").read_text(), launches)

    def test_missing_malformed_and_removed_transition_sources_require_reselection(self):
        name, _, selected = self.save_archive()
        archive = self.root / "state/archives" / (selected["archive_id"] + ".json")
        original = json.loads(archive.read_text())
        original["layout_transition"] = None
        removed = dict(original)
        del removed["layout_transition"]
        for at in (2, 3):
            for replacement in (None, "{malformed", json.dumps(removed)):
                with self.subTest(at=at, replacement=replacement):
                    archive.write_text(json.dumps(original))
                    self.replace_on_writer_read(archive, replacement, at)
                    result = subprocess.run([str(pause.HGS), "resume", name, "--archive", selected["archive_id"], "-d"],
                                            env=self.env, capture_output=True, text=True, timeout=20)
                    self.assertTrue((self.root / "archive-replaced.json").is_file(), result.stdout + result.stderr)
                    self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
                    self.assertIn("select the archive again", result.stderr)
                    self.assertEqual(archive.read_text() if archive.exists() else None, replacement)
                    self.assertFalse(self.binding(name))

    def test_unchanged_archive_restores_with_legacy_layout_and_original_identity(self):
        name, _, selected = self.save_archive()
        archive = self.root / "state/archives" / (selected["archive_id"] + ".json")
        original = json.loads(archive.read_text())
        original.update(layout="id", terminal_name="previous-terminal",
                        layout_transition={"opaque": {"must": "retain"}})
        archive.write_text(json.dumps(original))
        # Reordered JSON and insignificant whitespace are not a source change.
        replacement = json.dumps(dict(reversed(list(original.items()))), indent=3)
        self.replace_on_writer_read(archive, replacement, at=3)
        self.hgs("resume", name, "--archive", selected["archive_id"], "-d")
        self.wait(lambda: self.binding(name).get("activity") == "idle" and
                  self.binding(name).get("conversation_id") == original["conversation_id"])
        restored = self.binding(name)
        self.assertEqual(restored["session_id"], original["session_id"])
        self.assertNotEqual(restored["run_id"], original["run_id"])
        self.assertEqual(restored["layout_transition"], original["layout_transition"])
        self.assertNotIn("layout", restored)
        self.assertEqual(restored["terminal_name"], name)
        self.assertFalse(archive.exists(), "only exact native confirmation retires the archive")


if __name__ == "__main__":
    unittest.main()
