"""Dormant task controls preserve the current real-CLI admission contract.

Only a scripted agent in DeliveryFixture's private tmux server is used. The
connector's native call is `task mobile-send --json`, exercised directly here.
"""
import json
import sqlite3
import subprocess

from tests import test_tasks


class InactiveControls(test_tasks.DeliveryFixture):
    def test_an_active_managed_task_does_not_block_person_or_phone_send(self):
        # Dormant code must not inspect or repair task-control state, even if
        # a future writer left a version this binary would have to refuse.
        desktop = self.state / "desktop"
        desktop.mkdir(mode=0o700)
        path = desktop / "state.sqlite3"
        with sqlite3.connect(path) as db:
            db.execute("CREATE TABLE desktop_task_control_meta(id PRIMARY KEY,version)")
            db.execute("INSERT INTO desktop_task_control_meta VALUES(1,99)")
        path.chmod(0o600)
        before = path.read_bytes()

        first = self.add("Original foreground task", "--target", self.name)
        sent = self.hgs("send", first["ref"])
        self.wait_received(first["text"])
        self.confirm()
        self.assertGreater(sent["task"]["execution"]["attempts"], 0)
        self.assertEqual(sent["task"]["stage"], "active")
        record = json.loads(self.record_path.read_text())
        record.update(activity="busy", phase="working")
        self.record_path.write_text(json.dumps(record))

        second = self.add("Another person sends work", "--target", self.name)
        person = self.hgs("send", second["ref"])
        self.assertEqual(person["receipt"]["status"], "submitted")
        self.wait_received(second["text"])
        self.confirm()

        third = self.add("Phone sends work while another task is active")
        request = {"schema": 1, "project": "project-1", "task": third["uuid"], "session": self.name}
        result = subprocess.run([str(test_tasks.HGS), "task", "mobile-send", "--json"],
                                input=json.dumps(request), env=self.env, text=True,
                                capture_output=True, timeout=60)
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        answer = json.loads(result.stdout)
        receipt = json.loads((self.state / "input_receipts" / (answer["receipt"]["request_id"] + ".json")).read_text())
        expected = {"schema": 1, "ok": True, "task": third["uuid"], "stage": "active",
                    "session": person["task"]["session"],
                    "receipt": {key: receipt[key] for key in ("request_id", "status", "name", "run_id", "conversation_id", "submitted_at")}}
        # Full existing answer bytes: no task-control keys, altered receipt or
        # synthetic occupied-slot refusal in the native connector contract.
        self.assertEqual(result.stdout, json.dumps(expected, sort_keys=True, separators=(",", ":")) + "\n")
        self.wait_received(third["text"])
        self.assertEqual(path.read_bytes(), before)
        for task in (first, second, third):
            self.assertEqual(self.hgs("get", task["ref"])["task"]["stage"], "active")
