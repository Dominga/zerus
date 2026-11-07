"""Keep cache warm: setting, worker round and silent ping turns, with private fake agents only."""
import fcntl
import json
import subprocess
import time
import unittest

import test_input as fixtures

TEXT = "[Zerus keep-warm] Reply with exactly OK and do nothing else."


class KeepWarm(unittest.TestCase):
    tearDown = fixtures.InputTransport.tearDown

    def setUp(self):
        fixtures.InputTransport.setUp(self)
        # Hold the recovery executor lock: enabling keep-warm then starts no
        # detached worker, and `recovery tick` runs each round in the test.
        directory = self.state / "recovery"
        directory.mkdir(exist_ok=True)
        self.worker_lock = (directory / "worker.lock").open("a")
        fcntl.flock(self.worker_lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        self.addCleanup(self.worker_lock.close)

    script = fixtures.InputTransport.script
    tmux = fixtures.InputTransport.tmux
    wait_for = fixtures.InputTransport.wait_for
    write_record = fixtures.InputTransport.write_record
    screen = fixtures.InputTransport.screen
    received = fixtures.InputTransport.received

    def command(self, *args, payload=None, ok=True):
        result = subprocess.run([str(fixtures.HGS), *args], env=self.env, text=True, capture_output=True,
                                input=json.dumps(payload) if payload is not None else None, timeout=15)
        if ok:
            self.assertEqual(result.returncode, 0, result.stderr + result.stdout)
        return json.loads(result.stdout) if result.stdout.strip() else result.stderr

    def change(self, revision=0, ok=True, enabled=True, **duration):
        body = dict(expected_run_id=self.run_id, expected_conversation_id=self.conversation_id,
                    expected_revision=revision, enabled=enabled, **duration)
        return self.command("keep-warm", self.name, "--json", payload=body, ok=ok)

    def inspect(self):
        return self.command("inspect", self.name)

    def listed(self):
        rows = self.command("ls", "--local", "--json")["sessions"]
        return next(row for row in rows if row["name"] == self.name)

    def test_setting_is_revisioned_listed_and_turned_off(self):
        before = time.time()
        changed = self.change(hours=3)
        self.assertTrue(changed["ok"] and changed["applied"], changed)
        warm = changed["keep_warm"]
        self.assertTrue(warm["enabled"])
        self.assertGreaterEqual(warm["until"], before + 3 * 3600)
        self.assertLessEqual(warm["until"], time.time() + 3 * 3600)
        listed = self.listed()["keep_warm"]
        self.assertEqual((listed["enabled"], listed["revision"], listed["turn_active"]), (True, 1, False))
        self.assertEqual(self.inspect()["keep_warm"]["revision"], 1)
        # A second desktop with the old revision is refused, not merged.
        conflict = self.change(revision=0, hours=5, ok=False)
        self.assertEqual(conflict["error"], "conflict")
        always = self.change(revision=1, always=True)
        self.assertIsNone(always["keep_warm"]["until"])
        off = self.change(revision=2, enabled=False)
        self.assertFalse(off["keep_warm"]["enabled"])
        self.assertEqual(self.listed()["keep_warm"]["state"], "off")

    def test_a_stale_conversation_is_refused(self):
        stale = self.command("keep-warm", self.name, "--json", ok=False, payload=dict(
            expected_run_id=self.run_id, expected_conversation_id="other", expected_revision=0,
            enabled=True, hours=1))
        self.assertEqual(stale["error"], "stale_identity")
        self.assertFalse((self.state / "keep-warm.json").exists())


if __name__ == "__main__":
    unittest.main()
