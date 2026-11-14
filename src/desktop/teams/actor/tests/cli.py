"""Run the adapter's compiled argv against the real CLI in private task state.

HGS_TEST_BIN selects this source's normal CLI; HGS_TEST_ACTOR_BINARY selects
its Rust unit-test executable. The explicit command matrix exercises delivery, recovery, finish, stage and
clear, with only native agent input replaced by the test callback.
"""
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

HGS = Path(os.environ["HGS_TEST_BIN"])


class AdapterCommand(unittest.TestCase):
    def setUp(self):
        temp = tempfile.TemporaryDirectory(prefix="teams-actor-cli-")
        self.addCleanup(temp.cleanup)
        root = Path(temp.name)
        tmux = root / "tmux"
        tmux.mkdir(mode=0o700)
        self.env = dict(TMUX_TMPDIR=str(tmux), PATH=os.environ.get("PATH", "/usr/bin:/bin"),
                        HOME=str(root), HGS_CONFIG_DIR=str(root / "config"),
                        HGS_STATE_DIR=str(root / "state"), HGS_SELF="fixture")
        catalog = {"projects": [{"id": "project-1", "name": "example", "color": "#123456",
                                  "folders": [], "sessions": []}]}
        initialized = self.run_child([str(HGS), "swarm", "initialize"], input=json.dumps(catalog),
                                     env=self.env, text=True, capture_output=True, timeout=30)
        self.assertEqual(initialized.returncode, 0, initialized.stderr)

    def run_child(self, argv, **kwargs):
        env = kwargs.setdefault("env", self.env)
        root = Path(self.env["HOME"]) / "tmux"
        self.assertEqual(env.get("TMUX_TMPDIR"), str(root))
        self.assertTrue(root.is_dir())
        self.assertEqual(root.stat().st_mode & 0o777, 0o700)
        self.assertNotIn("TMUX", env)
        self.assertNotEqual(argv[1:3], ["task", "team-deliver"],
                            "Delivery must use the compiled native-input fixture")
        return subprocess.run(argv, **kwargs)

    def test_child_tmux_directory_is_private_and_exists(self):
        result = self.run_child(
            [sys.executable, "-c", "import json,os,pathlib; p=pathlib.Path(os.environ['TMUX_TMPDIR']); "
             "print(json.dumps([str(p), p.is_dir(), p.stat().st_mode & 0o777, 'TMUX' in os.environ]))"],
            env=self.env, text=True, capture_output=True, timeout=5,
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        root, exists, mode, inherited = json.loads(result.stdout)
        self.assertEqual(Path(root), Path(self.env["HOME"]) / "tmux")
        self.assertTrue(exists)
        self.assertEqual(mode, 0o700)
        self.assertFalse(inherited)

    def test_exact_occupancy_arguments(self):
        runner = os.environ["HGS_TEST_ACTOR_BINARY"]
        reply = self.run_child(
            [runner, "desktop::teams::actor::tests::native::adapter_argument_vectors_use_the_shared_builders",
             "--exact", "--nocapture", "--test-threads=1"],
            env=self.env, text=True, capture_output=True, timeout=30, check=True,
        )
        marker = "TEAM_ACTOR_ARGUMENTS="
        start = reply.stdout.index(marker) + len(marker)
        vectors, _ = json.JSONDecoder().raw_decode(reply.stdout[start:])
        result = self.run_child([str(HGS), *vectors["occupancy"]], env=self.env,
                                text=True, capture_output=True, timeout=30)
        self.assertEqual(result.returncode, 0, result.stderr)
        value = json.loads(result.stdout)
        self.assertEqual(value["damaged_total"], 0)
        self.assertEqual(value["tasks"], [])
        print("ACTOR_CLI_ARGV", json.dumps(vectors), flush=True)

    def test_task_command_matrix(self):
        result = self.run_child(
            [os.environ["HGS_TEST_ACTOR_BINARY"],
             "desktop::teams::actor::tests::commands::entry", "--exact", "--nocapture", "--test-threads=1"],
            env={**self.env, "HGS_TEST_BIN": str(HGS), "HGS_ACTOR_COMMAND_MODE": "matrix"},
            text=True, capture_output=True, timeout=90,
        )
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertIn("REAL_COMMAND package10 Native::clear => Cleared", result.stderr)
        print(result.stdout + result.stderr, flush=True)


if __name__ == "__main__":
    unittest.main(verbosity=2)
