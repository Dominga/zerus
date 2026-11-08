"""`hgs path-info`: one metadata read in a private home, and its guarded peer call (zerus-5wmh.13.18)."""
import json
import os
import shlex
from pathlib import Path
import subprocess
import tempfile
import unittest

REPO = Path(__file__).resolve().parents[1]
HGS = Path(os.environ.get("HGS_TEST_BIN", REPO / "hgs")).resolve()
MARKER = "hgs [@host] path-info --path"


class PathInfo(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="zerus-path-info-")
        self.root = Path(self.temp.name)
        self.addCleanup(self.temp.cleanup)
        self.env = {k: v for k, v in os.environ.items() if not k.startswith("HGS_")}
        self.env.update(HOME=str(self.root), USERPROFILE=str(self.root), XDG_CONFIG_HOME=str(self.root / "config"),
                        XDG_STATE_HOME=str(self.root / "state"), XDG_DATA_HOME=str(self.root / "data"),
                        HGS_CONFIG_DIR=str(self.root / "hgs-config"), HGS_STATE_DIR=str(self.root / "hgs-state"))

    def hgs(self, *args, rc=0):
        result = subprocess.run([str(HGS), *args], env=self.env, cwd=self.root,
                                capture_output=True, text=True, timeout=15)
        self.assertEqual(result.returncode, rc, result.stdout + result.stderr)
        return result

    def info(self, path):
        return json.loads(self.hgs("path-info", "--path", path).stdout)

    def test_a_file_a_folder_and_a_missing_path_read_no_content(self):
        notes = self.root / "notes"
        notes.mkdir()
        plan = notes / "plan.md"
        plan.write_bytes(b"twelve bytes")
        before = plan.stat()
        self.assertEqual(self.info(str(plan)), {"schema": 1, "exists": True, "kind": "file", "size": 12})
        self.assertEqual(self.info("~/notes/plan.md"), {"schema": 1, "exists": True, "kind": "file", "size": 12})
        self.assertEqual(self.info(str(notes)), {"schema": 1, "exists": True, "kind": "directory", "size": None})
        self.assertEqual(self.info(str(notes / "gone.md")), {"schema": 1, "exists": False, "kind": None, "size": None})
        # Nothing was read or changed: the access and change times stay.
        after = plan.stat()
        self.assertEqual((after.st_atime_ns, after.st_mtime_ns, after.st_ctime_ns),
                         (before.st_atime_ns, before.st_mtime_ns, before.st_ctime_ns))
        self.assertEqual(sorted(p.name for p in self.root.iterdir()), ["notes"], "no state or config was written")

    def test_a_relative_or_malformed_path_and_a_malformed_call_are_refused(self):
        self.assertIn("absolute path", self.hgs("path-info", "--path", "notes/plan.md", rc=1).stderr)
        self.assertIn("control characters", self.hgs("path-info", "--path", "/srv/a\u0007b", rc=1).stderr)
        self.assertIn("usage: hgs [@host] path-info --path <path>", self.hgs("path-info", rc=2).stderr)
        self.assertIn("usage: hgs [@host] path-info --path <path>", self.hgs("path-info", "/srv/a", rc=2).stderr)

    def test_the_help_names_it_once_and_a_peer_read_needs_no_terminal(self):
        # The exact marker a client and a forwarding hgs look for (test_remote_login_shells checks the guard).
        self.assertEqual(self.hgs("--help").stdout.count(MARKER), 1)
        dry = self.hgs("--dry-run", "@fixture.invalid", "path-info", "--path", "~/plan.md")
        self.assertIn("BatchMode=yes", dry.stdout)
        self.assertNotIn(" -t ", dry.stdout, "no terminal for a read")
        # The peer's hgs runs only once its help names the command, and gets ~ unexpanded.
        remote = shlex.split(dry.stdout)[-3:]
        self.assertEqual(remote[:2], ["sh", "-c"])
        self.assertIn(f"grep -qF '{MARKER}'", remote[2])
        self.assertIn("exec ~/.local/bin/hgs path-info --path '~/plan.md' --dry-run", remote[2])
        self.assertIn("no --dry-run", self.hgs("path-info", "--path", "/srv/plan.md", "--dry-run", rc=1).stderr)


if __name__ == "__main__":
    unittest.main()
