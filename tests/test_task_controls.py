"""Dormant task controls preserve the current real-CLI admission contract.

Only a scripted agent in DeliveryFixture's private tmux server is used. The
connector's native call is `task mobile-send --json`, exercised directly here.
"""
import json
import hashlib
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


class InactiveControlCommands(test_tasks.Fixture):
    def call(self, args, payload=None, code="task_controls_inactive"):
        result = subprocess.run([str(test_tasks.HGS), "task", *args],
                                input=None if payload is None else json.dumps(payload),
                                env=self.env, text=True, capture_output=True, timeout=30)
        self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
        answer = json.loads(result.stdout)
        self.assertEqual((answer["schema"], answer["ok"], answer["code"], answer["attempted"]),
                         (1, False, code, False))
        return answer

    def test_new_commands_remain_inactive_without_opening_task_or_desktop_stores(self):
        original = "00000000-0000-4000-8000-000000000001"
        desktop = self.state / "desktop"
        desktop.mkdir(mode=0o700)
        path = desktop / "state.sqlite3"
        with sqlite3.connect(path) as db:
            db.execute("CREATE TABLE desktop_task_control_meta(id PRIMARY KEY,version)")
            db.execute("INSERT INTO desktop_task_control_meta VALUES(1,99)")
        path.chmod(0o600)
        before = path.read_bytes()
        answer = self.call(["begin-attempt", "missing/T-234567", "--attempt", original, "--json"])
        self.assertEqual(answer["request_id"], original)
        plan = {"schema": 1, "request_id": original, "action": "pause",
                "target": {"scope": "attempt", "project": "not-resolved", "task": original, "attempt": original},
                "observed_pauses": [], "observed_stops": []}
        self.assertEqual(self.call(["control", "plan", "--json"], plan)["request_id"], original)
        executor = {"computer": original, "session_id": original, "run": "original-run", "birth": "original-birth", "conversation": "original-conversation", "child": None}
        execute = {"schema": 1, "request": {"id": original, "action": "pause", "executor": executor,
                   "attempt": {"project": "not-resolved", "task": original, "attempt": original,
                               "generation": 1, "fingerprint": "a" * 64},
                   "slot": {"id": original, "revision": 1}, "task_version": "original-version", "turn": "original-turn",
                   "observed_pauses": [], "observed_stops": []}, "caller": {"kind": "person"},
                   "configuration": "b" * 64, "store_identity": "c" * 64, "cli_image": "d" * 64, "loaded_adapter": "e" * 64}
        encoded = json.dumps(execute, sort_keys=True, ensure_ascii=False, separators=(",", ":")).encode("utf-8")
        execute["request"]["admission"] = hashlib.sha256(b"zerus-task-control-admission-v1\0" + encoded).hexdigest()
        self.assertEqual(self.call(["control", "execute", "--json"], execute)["request_id"], original)
        execute["caller"]["surprise"] = 1
        self.call(["control", "execute", "--json"], execute, code="invalid_task_control")
        status = {"schema": 1, "target": {"scope": "request", "request_id": original}}
        self.assertEqual(self.call(["control", "status", "--json"], status)["request_id"], original)
        self.assertEqual(path.read_bytes(), before)
        help_text = subprocess.check_output([str(test_tasks.HGS), "--help"], env=self.env, text=True)
        self.assertIn("hgs task begin-attempt", help_text)
        self.assertNotIn("task-control-v1", help_text.splitlines())

    def test_bad_origin_and_spoofed_actor_are_refused_before_any_effect(self):
        self.call(["begin-attempt", "p/T-234567", "--attempt", "latest", "--json"], code="invalid_task_control")
        original = "00000000-0000-4000-8000-000000000001"
        payload = {"schema": 1, "target": {"scope": "request", "request_id": original}, "actor": "person"}
        self.call(["control", "status", "--json"], payload, code="invalid_task_control")
