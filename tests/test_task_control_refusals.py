"""A permanent negative native-input reservation never becomes absence.

Only existing CLI operations are used. The same maintained control is also
run against a frozen current-main binary before adding the new writer.
"""
import hashlib
import json
import subprocess

from tests import test_tasks


def encoded(value):
    return json.dumps(value, sort_keys=True, ensure_ascii=False,
                      separators=(",", ":")).encode("utf-8")


class ReceiptReaders(test_tasks.DeliveryFixture):
    start_plan = test_tasks.TaskDelivery.start_plan

    def test_the_closed_gate_omits_mode_keeps_the_prompt_and_writes_no_negative(self):
        task = self.add("A closed-gate Start", "--target", self.name)
        plan = self.start_plan(task)
        self.assertNotIn("acknowledgement_mode", plan)
        self.assertNotIn("begin-attempt", plan["request"]["text"])
        self.assertIn("hgs task report-attempt", plan["request"]["text"])
        receipt = self.state / "input_receipts" / (plan["id"] + ".json")
        self.assertFalse(receipt.exists())
        for mode in [None, "managed", "legacy"]:
            changed = dict(plan, acknowledgement_mode=mode)
            self.assertIn("invalid task start plan", self.hgs(
                "start-deliver", task["ref"], text=json.dumps(changed), ok=False))
            self.assertFalse(receipt.exists())
        result = self.hgs("start-deliver", task["ref"], text=json.dumps(plan))
        self.assertEqual(result["state"], "submitted", result)
        self.assertNotIn("refusal", result)
        stored = json.loads(receipt.read_bytes())
        self.assertEqual(stored["status"], "submitted")
        self.assertNotIn("refusal_schema", stored)
        self.assertNotIn("acknowledgement_mode", stored)
        self.wait_received("A closed-gate Start")

    def test_a_refused_before_input_record_is_unknown_not_absent_to_old_readers(self):
        task = self.add("A task whose request is permanently reserved", "--target", self.name)
        plan = self.start_plan(task)
        request = plan["request"]
        directory = self.state / "input_receipts"
        directory.mkdir(mode=0o700, exist_ok=True)
        path = directory / (plan["id"] + ".json")
        self.assertFalse(path.exists())
        receipt = dict(refusal_schema=1, request_id=plan["id"], name=self.name,
                       run_id=request["expected_run_id"],
                       conversation_id=request["expected_conversation_id"],
                       first_message=not request["expected_conversation_id"],
                       digest=hashlib.sha256(encoded(request)).hexdigest(),
                       status="refused_before_input",
                       plan_digest=hashlib.sha256(b"zerus-task-start-refusal-v1\0" + encoded(plan)).hexdigest(),
                       reason="acknowledgement_mode_changed",
                       planned_mode="managed", admitted_mode="legacy")
        original = encoded(receipt)
        path.write_bytes(original)
        path.chmod(0o600)
        for options in [(), ("--recover-only",)]:
            result = self.hgs("start-deliver", task["ref"], *options, text=json.dumps(plan))
            self.assertEqual(result["state"], "uncertain", result)
            self.assertNotIn("refusal", result)
            self.assertEqual(path.read_bytes(), original)
            self.assertEqual(self.received(), "")
        # Direct native submission also sees a PRESENT reservation. It must
        # not get as far as terminal validation/input, even for changed text.
        for changed in [False, True]:
            payload = dict(request)
            if changed:
                payload["text"] = "Different text cannot recycle the UUID"
            result = subprocess.run([str(test_tasks.HGS), "send", self.name, "--json"],
                                    input=encoded(payload), env=self.env,
                                    capture_output=True, timeout=30)
            self.assertNotEqual(result.returncode, 0, result.stdout)
            if not changed:
                self.assertIn(b"delivery uncertain", result.stderr)
                self.assertNotIn(b"different content", result.stderr)
            self.assertEqual(path.read_bytes(), original)
            self.assertEqual(self.received(), "")
        current = self.hgs("get", task["ref"])["task"]
        self.assertEqual(current["stage"], "todo")
        self.assertFalse((current.get("execution") or {}).get("attempts"))
