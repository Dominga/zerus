"""The explicit local-copy command, with private catalogs and no agent process."""
import hashlib
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

HGS = Path(os.environ.get("HGS_TEST_BIN", Path(__file__).resolve().parents[1] / "target/debug/hgs")).resolve()


class TaskCopyReview(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory(prefix="task-copy-review-")
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.env = {k: v for k, v in os.environ.items() if not k.startswith("HGS_")}
        self.env.update(HOME=str(self.root), HGS_CONFIG_DIR=str(self.root / "config"), HGS_STATE_DIR=str(self.root / "state"))
        self.run_hgs("swarm", "initialize", text=json.dumps({"projects": [{"id": "example", "name": "orchard", "folders": [], "sessions": []}]}))
        self.task = self.run_hgs("task", "add", "--project", "orchard", text="Local copy control")["task"]
        self.folder = self.root / "state/replica" / hashlib.sha256(b"example").hexdigest() / "tasks"
        self.uuid = self.task["uuid"]

    def run_hgs(self, *args, text=None, ok=True):
        result = subprocess.run([str(HGS), *args], env=self.env, input=text, text=True, capture_output=True, timeout=12)
        if ok:
            self.assertEqual(result.returncode, 0, result.stderr)
            return json.loads(result.stdout)
        self.assertNotEqual(result.returncode, 0, result.stdout)
        return result.stderr

    def review(self, *args, ok=True):
        return self.run_hgs("task", "review-copy", "--project", "orchard", "--uuid", self.uuid, "--json", *args, ok=ok)

    def damage(self):
        (self.folder / f"{self.uuid}.loro").write_bytes(b"damaged snapshot")
        (self.folder / f"{self.uuid}.managed").write_bytes(b"unreadable managed marker")

    def test_preview_confirmation_and_replay_refusal_preserve_other_tasks(self):
        self.review(ok=False)  # The new task is healthy.
        other = self.run_hgs("task", "add", "--project", "orchard", text="Other task")["task"]["uuid"]
        kept = {p.name: p.read_bytes() for p in self.folder.glob(f"{other}.*")}
        self.damage()
        before = {p.name: p.read_bytes() for p in self.folder.glob(f"{self.uuid}.*")}
        preview = self.review()
        self.assertEqual(preview["state"], "review")
        self.assertEqual(before, {p.name: p.read_bytes() for p in self.folder.glob(f"{self.uuid}.*")})
        refused = self.review("--confirm", "0" * 64)
        self.assertEqual(refused["state"], "refused")
        moved = self.review("--confirm", preview["fingerprint"])
        self.assertEqual(moved["state"], "moved")
        backup = Path(moved["backup"])
        self.assertEqual((backup / f"{self.uuid}.loro").read_bytes(), before[f"{self.uuid}.loro"])
        self.assertFalse((self.folder / f"{self.uuid}.managed").exists())
        self.assertEqual(self.review("--confirm", preview["fingerprint"])["state"], "refused")
        self.assertEqual(len(list(self.folder.glob(f"{self.uuid}.review-*"))), 1)
        self.assertEqual(kept, {p.name: p.read_bytes() for p in self.folder.glob(f"{other}.*")})

    @unittest.skipUnless(os.name == "posix", "private flock control")
    def test_busy_writer_and_symlink_are_closed_refusals(self):
        import fcntl
        self.damage()
        with (self.folder.parent / ".lock").open("r+b") as lock:
            fcntl.flock(lock, fcntl.LOCK_EX)
            self.assertIn("busy", self.review(ok=False))
        outside = self.root / "outside"
        outside.write_bytes(b"unchanged")
        update = self.folder / f"{self.uuid}.updates"
        update.unlink(missing_ok=True)
        update.symlink_to(outside)
        self.review(ok=False)
        self.assertEqual(outside.read_bytes(), b"unchanged")
        self.assertTrue(update.is_symlink())

    def test_unexpected_or_duplicate_options_never_change_the_copy(self):
        self.damage()
        before = {p.name: p.read_bytes() for p in self.folder.glob(f"{self.uuid}.*")}
        for args in [("--uuid", self.uuid), ("--confirm", "invalid"), ("--unexpected",)]:
            self.review(*args, ok=False)
        self.assertEqual(before, {p.name: p.read_bytes() for p in self.folder.glob(f"{self.uuid}.*")})


if __name__ == "__main__":
    unittest.main()
