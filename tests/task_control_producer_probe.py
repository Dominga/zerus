"""Explicit const-on negative probe; never counts a skipped/off build as proof.

Run only with HGS_TEST_BIN pointing to the separately built private source copy
whose sole gate edit enables TASK_CONTROLS. No provider, GUI or owner state is used.
"""
from contextlib import closing
import json
import os
import sqlite3
import subprocess
import unittest
import uuid

import test_desktop as kernel


@unittest.skipIf(os.name == "nt", "Unix peer probe; not Windows qualification")
class ProducerRefusals(unittest.TestCase):
    cli = kernel.Desktop.cli
    serve = kernel.Desktop.serve
    stop = kernel.Desktop.stop

    def setUp(self):
        kernel.Desktop.setUp(self)
        help_text = subprocess.run([str(kernel.HGS), "--help"], env=self.env,
                                   capture_output=True, text=True, timeout=10)
        self.assertEqual(help_text.returncode, 0, help_text.stderr)
        self.assertIn("task-control-v1", help_text.stdout,
                      "this explicit probe requires the actual isolated const-on build")
        self.process, service = self.serve()
        self.assertIn("task_controls", service)
        self.before = self.dump()

    def dump(self):
        with closing(sqlite3.connect(self.desktop / "state.sqlite3")) as db:
            return list(db.iterdump())

    def unchanged(self):
        self.assertEqual(self.dump(), self.before, "no task/control/hold/request write")
        for path in [self.state / "task-stop-facts", self.state / "input_receipts",
                     self.state / "sessions", self.state / "ids", self.state / "teams",
                     self.config / "swarm", self.config / "teams",
                     self.root / "unexpected-tool"]:
            self.assertFalse(path.exists(), str(path))
        self.assertIsNone(self.process.poll())

    def call(self, args, payload=None, code="task_control_native_turn_association_unqualified"):
        result = self.cli("task", *args, data=payload, ok=False)
        reply = json.loads(result.stdout)
        self.assertEqual(reply["code"], code, reply)
        self.assertFalse(reply["ok"])
        self.assertFalse(reply["attempted"])
        self.assertNotIn("binding", reply)
        self.assertNotIn("plan", reply)
        self.assertNotIn("refusal", reply)
        if code == "task_control_native_turn_association_unqualified":
            self.assertTrue(reply["read_only"])
            self.assertIn("input generation", reply["error"])
        return reply

    def plan(self, action="yield"):
        return dict(schema=1, request_id=str(uuid.uuid4()), action=action,
                    target=dict(scope="attempt", project="orchard",
                                task=str(uuid.uuid4()), attempt=str(uuid.uuid4())),
                    observed_pauses=[], observed_stops=[])

    def test_begin_and_repeated_original_offer_no_binding_or_acknowledgement(self):
        attempt = str(uuid.uuid4())
        for _ in range(2):
            self.call(["begin-attempt", "orchard/T-234567", "--attempt", attempt, "--json"])
        self.unchanged()

    def test_yield_plan_has_no_slot_request_hold_or_task_side_effect(self):
        plan = self.plan()
        reply = self.call(["control", "plan", "--json"], plan)
        self.assertEqual(reply["request_id"], plan["request_id"])
        self.unchanged()

    def test_person_admission_remains_before_the_producer_gap(self):
        for action in ("pause", "resume"):
            reply = self.call(["control", "plan", "--json"], self.plan(action),
                              code="task_control_admission_refused")
            self.assertEqual(reply["error"],
                             "Person task controls require a separately verified native GUI action")
        self.unchanged()

    def test_serialized_turn_generation_or_actor_does_not_create_a_producer(self):
        for key, value in [("turn", "claimed-native-turn"), ("input_generation", 7),
                           ("caller", {"kind": "executor"}), ("actor", "person")]:
            plan = self.plan()
            plan[key] = value
            self.call(["control", "plan", "--json"], plan, code="invalid_task_control")
        self.unchanged()
